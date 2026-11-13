//! Умный HTTP-прокси 127.0.0.1:8081: маршрутизация по доменам между
//! WireGuard (wireproxy), VLESS/SS (xray) и прямым соединением.
//! Паритет с legacy manager.js: CONNECT-цепочка в upstream, plain-HTTP
//! absolute-URI форвардинг, фильтр hop-by-hop заголовков, журнал direct-сайтов.
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use url::Url;

use crate::net::dns_guard;
use crate::routing::Channel;
use crate::state::AppState;
use crate::vless::{WIREPROXY_HTTP_PORT, XRAY_HTTP_PORT};
use crate::wlog;

const HEAD_LIMIT: usize = 65536;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

pub async fn serve(state: Arc<AppState>) -> Result<()> {
    let listener = TcpListener::bind(("127.0.0.1", crate::api::SMART_PROXY_PORT)).await?;
    wlog!(
        "умный прокси: 127.0.0.1:{} (режим {})",
        crate::api::SMART_PROXY_PORT,
        crate::lists::MODE
    );
    loop {
        let (sock, _) = listener.accept().await?;
        let st = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(sock, st).await {
                wlog!("прокси: {e}");
            }
        });
    }
}

async fn handle(mut client: TcpStream, state: Arc<AppState>) -> Result<()> {
    let (head, rest) = read_head(&mut client).await.context("чтение заголовка")?;
    let text = String::from_utf8_lossy(&head).to_string();
    let mut lines = text.split("\r\n");
    let req_line = lines.next().unwrap_or("");
    let mut parts = req_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("").to_string();

    let mut headers: Vec<(String, String)> = Vec::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }

    if method.eq_ignore_ascii_case("CONNECT") {
        return tunnel(client, state, &target, rest).await;
    }
    if target.to_ascii_lowercase().starts_with("http://") {
        return forward_http(client, state, &method, &target, &headers, rest).await;
    }
    client
        .write_all(
            b"HTTP/1.1 400 Bad Request\r\nContent-Type: text/plain\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        )
        .await?;
    Ok(())
}

async fn tunnel(
    mut client: TcpStream,
    state: Arc<AppState>,
    target: &str,
    rest: Vec<u8>,
) -> Result<()> {
    let (host, port) = parse_hostport(target, 443)?;
    reject_forbidden(&host)?;

    let routing = state.routing_snapshot();
    let route = routing.route_for(&host);
    if route.is_none() {
        state.visited.lock().unwrap().note(&host);
    }
    let route_label = route
        .map(|c| match c {
            Channel::Wireguard => "wireguard",
            Channel::Vless => "vless",
        })
        .unwrap_or("direct");
    wlog!("CONNECT {host}:{port} [VPN:{route_label}]");

    let mut upstream = match route {
        Some(ch) => connect_proxy_tunnel(ch, target).await?,
        None => direct_connect(&host, port).await?,
    };
    client
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await?;
    if !rest.is_empty() {
        upstream.write_all(&rest).await?;
    }
    let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
    Ok(())
}

async fn forward_http(
    mut client: TcpStream,
    state: Arc<AppState>,
    method: &str,
    target: &str,
    headers: &[(String, String)],
    rest: Vec<u8>,
) -> Result<()> {
    let url = Url::parse(target).context("некорректный URL")?;
    if url.scheme() != "http" {
        bail!("plain-прокси принимает только http:// (https — через CONNECT)");
    }
    let host = url.host_str().context("нет хоста в URL")?.to_string();
    let port = url.port_or_known_default().unwrap_or(80);
    reject_forbidden(&host)?;

    let routing = state.routing_snapshot();
    let route = routing.route_for(&host);
    if route.is_none() {
        state.visited.lock().unwrap().note(&host);
    }
    let route_label = route
        .map(|c| match c {
            Channel::Wireguard => "wireguard",
            Channel::Vless => "vless",
        })
        .unwrap_or("direct");
    let path = match route {
        Some(_) => url.to_string(),
        None => match url.query() {
            Some(q) => format!("{}?{q}", url.path()),
            None => url.path().to_string(),
        },
    };
    wlog!("{method} {host} [VPN:{route_label}]");

    let mut out = format!("{method} {path} HTTP/1.1\r\n");
    let mut has_host = false;
    for (k, v) in headers {
        let lower = k.to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "proxy-connection"
                | "connection"
                | "keep-alive"
                | "te"
                | "trailer"
                | "transfer-encoding"
                | "upgrade"
        ) {
            continue;
        }
        if lower == "host" {
            has_host = true;
        }
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    if !has_host {
        out.push_str(&format!("Host: {host}\r\n"));
    }
    out.push_str("Connection: close\r\n\r\n");

    let mut upstream = match route {
        Some(ch) => {
            let port = match ch {
                Channel::Wireguard => WIREPROXY_HTTP_PORT,
                Channel::Vless => XRAY_HTTP_PORT,
            };
            TcpStream::connect(("127.0.0.1", port))
                .await
                .with_context(|| format!("upstream-прокси недоступен ({port})"))?
        }
        None => direct_connect(&host, port).await?,
    };
    upstream.write_all(out.as_bytes()).await?;
    if !rest.is_empty() {
        upstream.write_all(&rest).await?;
    }
    let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
    Ok(())
}

fn reject_forbidden(host: &str) -> Result<()> {
    if dns_guard::forbidden_hostname(host) {
        bail!("запрещённый адрес назначения: {host}");
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        if dns_guard::is_forbidden_ip(ip) {
            bail!("запрещённый адрес назначения: {host}");
        }
    }
    Ok(())
}

async fn connect_proxy_tunnel(channel: Channel, target: &str) -> Result<TcpStream> {
    let port = match channel {
        Channel::Wireguard => WIREPROXY_HTTP_PORT,
        Channel::Vless => XRAY_HTTP_PORT,
    };
    let mut stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(("127.0.0.1", port)))
        .await
        .context("таймаут подключения к upstream")?
        .with_context(|| format!("upstream-прокси не отвечает (порт {port})"))?;
    let request = format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n");
    stream.write_all(request.as_bytes()).await?;
    let (head, _) = read_head(&mut stream)
        .await
        .context("upstream: нет ответа")?;
    let status_line = String::from_utf8_lossy(&head)
        .lines()
        .next()
        .unwrap_or("")
        .to_string();
    if !status_line.contains(" 200") {
        bail!("upstream отказал: {status_line}");
    }
    Ok(stream)
}

async fn direct_connect(host: &str, port: u16) -> Result<TcpStream> {
    let addrs = dns_guard::resolve_checked(host).await?;
    let mut last_err: Option<String> = None;
    for ip in addrs {
        match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((ip, port))).await {
            Ok(Ok(stream)) => return Ok(stream),
            Ok(Err(e)) => last_err = Some(e.to_string()),
            Err(_) => last_err = Some("таймаут".into()),
        }
    }
    bail!(
        "не удалось подключиться к {host}:{port}: {}",
        last_err.unwrap_or_default()
    )
}

fn find_sub(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

async fn read_head(stream: &mut TcpStream) -> Result<(Vec<u8>, Vec<u8>)> {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        if let Some(pos) = find_sub(&buf, b"\r\n\r\n") {
            let end = pos + 4;
            return Ok((buf[..end].to_vec(), buf[end..].to_vec()));
        }
        if buf.len() > HEAD_LIMIT {
            bail!("заголовок слишком большой");
        }
        let n = tokio::time::timeout(Duration::from_secs(15), stream.read(&mut chunk))
            .await
            .context("таймаут чтения заголовка")??;
        if n == 0 {
            bail!("соединение закрыто до заголовка");
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

fn parse_hostport(target: &str, default_port: u16) -> Result<(String, u16)> {
    match target.rsplit_once(':') {
        Some((host, port)) => {
            let host = host.trim_matches(|c| c == '[' || c == ']').to_string();
            let port: u16 = port.parse().context("некорректный порт")?;
            Ok((host, port))
        }
        None => Ok((
            target.trim_matches(|c| c == '[' || c == ']').to_string(),
            default_port,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostport_parsing() {
        assert_eq!(
            parse_hostport("example.com:443", 443).unwrap(),
            ("example.com".into(), 443)
        );
        assert_eq!(
            parse_hostport("[::1]:8443", 443).unwrap(),
            ("::1".into(), 8443)
        );
        assert_eq!(
            parse_hostport("example.com", 443).unwrap(),
            ("example.com".into(), 443)
        );
    }

    #[test]
    fn head_finder() {
        assert_eq!(find_sub(b"abc\r\n\r\ndef", b"\r\n\r\n"), Some(3));
        assert_eq!(find_sub(b"abc", b"\r\n\r\n"), None);
    }
}
