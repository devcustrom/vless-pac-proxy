//! Плагин локальных доменов: HTTP :80 и HTTPS :443 (rustls, SNI), реверс-прокси
//! по Host на локальные порты, WebSocket-upgrade. Паритет с legacy local-domains.js.
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;

use crate::state::AppState;
use crate::wlog;

const HEAD_LIMIT: usize = 65536;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalDomain {
    pub domain: String,
    pub port: u16,
}

/// Формат строки: "domain = port" или "domain port".
pub fn parse_line(raw: &str) -> Option<LocalDomain> {
    let line = raw.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let mut parts = line.split(['=', ' ', '\t']).filter(|s| !s.is_empty());
    let domain = parts.next()?.to_lowercase();
    let port: u16 = parts.next()?.parse().ok()?;
    let domain = domain
        .trim_start_matches("http://")
        .trim_start_matches("https://");
    let domain = domain.split('/').next()?.split(':').next()?.to_string();
    if domain.is_empty() || port == 0 {
        return None;
    }
    Some(LocalDomain { domain, port })
}

pub fn parse_config(text: &str) -> Vec<LocalDomain> {
    let mut out: Vec<LocalDomain> = Vec::new();
    for line in text.lines() {
        if let Some(d) = parse_line(line) {
            if !out.iter().any(|x| x.domain == d.domain) {
                out.push(d);
            }
        }
    }
    out
}

fn sanitize(value: &str, limit: usize) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_graphic() || *c == ' ')
        .take(limit)
        .collect()
}

fn find_port(state: &AppState, domain: &str) -> Option<u16> {
    let domains = state.local_domains.lock().ok()?;
    domains.iter().find(|d| d.domain == domain).map(|d| d.port)
}

async fn read_head<S>(stream: &mut S) -> Result<(Vec<u8>, Vec<u8>)>
where
    S: AsyncRead + Unpin,
{
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

fn find_sub(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

async fn connect_local(port: u16) -> Result<TcpStream> {
    for host in ["127.0.0.1", "::1"] {
        let attempt =
            tokio::time::timeout(Duration::from_secs(5), TcpStream::connect((host, port)))
                .await
                .unwrap_or(Err(std::io::Error::other("timeout")));
        if let Ok(s) = attempt {
            return Ok(s);
        }
    }
    bail!("локальный сервис на порту {port} недоступен");
}

pub async fn serve(state: Arc<AppState>) -> Result<()> {
    let settings = state.settings_snapshot();
    // TLS: собираем стор сертификатов и кладём в общий слот (горячая замена)
    let domain_names: Vec<String> = state
        .local_domains
        .lock()
        .map(|d| d.iter().map(|x| x.domain.clone()).collect())
        .unwrap_or_default();
    if let Err(e) = crate::local_hosts::write_script(&state.dir, &domain_names) {
        wlog!("локальные домены: скрипт hosts — {e:#}");
    }
    // Слот TLS создаём всегда, даже без доменов: иначе добавленный позже домен
    // не сможет горячо подменить конфиг и HTTPS будет рвать соединения.
    match crate::local_tls::build_store(&state.dir, &domain_names) {
        Ok(store) => {
            let cfg = crate::local_tls::server_config(Arc::new(store));
            if let Ok(mut slot) = state.tls_config.lock() {
                *slot = Some(Arc::new(tokio::sync::RwLock::new(cfg)));
            }
            if let Ok(mut err) = state.local_https_error.lock() {
                *err = None;
            }
        }
        Err(e) => {
            if let Ok(mut err) = state.local_https_error.lock() {
                *err = Some(format!("{e:#}"));
            }
            wlog!("локальный HTTPS: сертификаты не созданы — {e:#}");
        }
    }

    // HTTP
    let http_state = state.clone();
    let http_port = settings.local_proxy_port;
    tokio::spawn(async move {
        match TcpListener::bind(("127.0.0.1", http_port)).await {
            Ok(listener) => {
                if let Ok(mut err) = http_state.local_http_error.lock() {
                    *err = None;
                }
                wlog!("локальные домены: http://127.0.0.1:{http_port}");
                loop {
                    if let Ok((sock, _)) = listener.accept().await {
                        let st = http_state.clone();
                        tokio::spawn(async move {
                            handle_stream(sock, st).await;
                        });
                    }
                }
            }
            Err(e) => {
                if let Ok(mut err) = http_state.local_http_error.lock() {
                    *err = Some(format!("порт {http_port} занят: {e}"));
                }
                wlog!("локальные домены: HTTP не запущен — {e}");
            }
        }
    });

    // HTTPS
    let https_state = state.clone();
    let https_port = settings.local_https_port;
    tokio::spawn(async move {
        let listener = match TcpListener::bind(("127.0.0.1", https_port)).await {
            Ok(l) => l,
            Err(e) => {
                if let Ok(mut err) = https_state.local_https_error.lock() {
                    *err = Some(format!("порт {https_port} занят: {e}"));
                }
                wlog!("локальные домены: HTTPS не запущен — {e}");
                return;
            }
        };
        wlog!("локальные домены: https://127.0.0.1:{https_port}");
        loop {
            if let Ok((sock, _)) = listener.accept().await {
                let st = https_state.clone();
                let cfg = https_state
                    .tls_config
                    .lock()
                    .ok()
                    .and_then(|slot| slot.clone());
                tokio::spawn(async move {
                    let Some(cfg) = cfg else { return };
                    let cfg_now = cfg.read().await.clone();
                    let acceptor = TlsAcceptor::from(cfg_now);
                    match acceptor.accept(sock).await {
                        Ok(tls) => handle_stream(tls, st).await,
                        Err(_) => { /* TLS-рукопожатие не прошло */ }
                    }
                });
            }
        }
    });
    Ok(())
}

async fn handle_stream<S>(mut stream: S, state: Arc<AppState>)
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let Ok((head, rest)) = read_head(&mut stream).await else {
        return;
    };
    let text = String::from_utf8_lossy(&head).to_string();
    let mut lines = text.split("\r\n");
    let req_line = lines.next().unwrap_or("");
    let mut parts = req_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();

    let mut headers: Vec<(String, String)> = Vec::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    let host = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("host"))
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    let domain = host.split(':').next().unwrap_or("").to_ascii_lowercase();
    let is_upgrade = headers.iter().any(|(k, v)| {
        k.eq_ignore_ascii_case("upgrade") && v.to_ascii_lowercase().contains("websocket")
    });

    let Some(port) = find_port(&state, &domain) else {
        let names = {
            let d = state
                .local_domains
                .lock()
                .map(|d| d.clone())
                .unwrap_or_default();
            d.iter()
                .map(|x| x.domain.clone())
                .collect::<Vec<_>>()
                .join(", ")
        };
        let body = format!(
            "локальный домен не настроен\nнастроены: {}",
            sanitize(&names, 300)
        );
        let response = format!(
            "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain; charset=utf-8\r\nX-Content-Type-Options: nosniff\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.shutdown().await;
        return;
    };

    let safe_host = sanitize(&domain, 128);
    let safe_path = sanitize(&target, 512);
    wlog!("local: {safe_host}{safe_path} -> :{port}");

    let mut upstream = match connect_local(port).await {
        Ok(u) => u,
        Err(e) => {
            let body = "upstream error";
            let response = format!(
                "HTTP/1.1 502 Bad Gateway\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes()).await;
            let _ = stream.shutdown().await;
            wlog!("local: {safe_host} — {e}");
            return;
        }
    };

    let mut out = format!("{method} {target} HTTP/1.1\r\n");
    let mut has_host = false;
    for (k, v) in &headers {
        let lower = k.to_ascii_lowercase();
        if !is_upgrade
            && matches!(
                lower.as_str(),
                "proxy-connection"
                    | "connection"
                    | "keep-alive"
                    | "te"
                    | "trailer"
                    | "transfer-encoding"
                    | "upgrade"
            )
        {
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
    if !is_upgrade {
        out.push_str("Connection: close\r\n");
    }
    out.push_str("\r\n");

    if upstream.write_all(out.as_bytes()).await.is_err() {
        return;
    }
    if !rest.is_empty() && upstream.write_all(&rest).await.is_err() {
        return;
    }
    let _ = tokio::io::copy_bidirectional(&mut stream, &mut upstream).await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lines() {
        assert_eq!(
            parse_line("app.localhost = 3000"),
            Some(LocalDomain {
                domain: "app.localhost".into(),
                port: 3000
            })
        );
        assert_eq!(
            parse_line("panel.localhost 8080"),
            Some(LocalDomain {
                domain: "panel.localhost".into(),
                port: 8080
            })
        );
        assert_eq!(parse_line("http://panel.localhost:8080/"), None);
        assert_eq!(parse_line("# comment"), None);
    }

    #[test]
    fn dedup_and_order() {
        let cfg = "a.localhost = 1\n# x\nb.localhost = 2\na.localhost = 3\n";
        let d = parse_config(cfg);
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].port, 1);
    }
}
