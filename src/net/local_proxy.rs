//! Плагин локальных доменов: HTTP :80 и HTTPS :443 (rustls, SNI), реверс-прокси
//! по Host на локальные порты, WebSocket-upgrade. Паритет с legacy local-domains.js.
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use futures_util::future::{select, Either};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;

use crate::state::AppState;
use crate::wlog;

const HEAD_LIMIT: usize = 65536;

/// Backend может слушать только 127.0.0.1 или только ::1 (часто второй).
/// Запоминаем рабочее семейство, чтобы не платить таймаутом на каждое соединение.
static PREFERRED_ADDR: OnceLock<Mutex<HashMap<u16, &'static str>>> = OnceLock::new();

fn preferred_addr() -> &'static Mutex<HashMap<u16, &'static str>> {
    PREFERRED_ADDR.get_or_init(|| Mutex::new(HashMap::new()))
}

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

async fn try_connect(host: &'static str, port: u16) -> Option<TcpStream> {
    tokio::time::timeout(Duration::from_secs(2), TcpStream::connect((host, port)))
        .await
        .ok()
        .and_then(|result| result.ok())
}

async fn connect_local(port: u16) -> Result<TcpStream> {
    // 1) пробуем уже известное семейство адреса — без задержек
    let cached = preferred_addr()
        .lock()
        .ok()
        .and_then(|map| map.get(&port).copied());
    if let Some(host) = cached {
        if let Some(stream) = try_connect(host, port).await {
            return Ok(stream);
        }
        if let Ok(mut map) = preferred_addr().lock() {
            map.remove(&port);
        }
    }

    // 2) оба семейства одновременно: если backend слушает только ::1,
    //    не ждём таймаут на 127.0.0.1
    let v4 = Box::pin(TcpStream::connect(("127.0.0.1", port)));
    let v6 = Box::pin(TcpStream::connect(("::1", port)));
    let chosen = tokio::time::timeout(Duration::from_secs(5), async {
        match select(v4, v6).await {
            Either::Left((result, v6)) => match result {
                Ok(stream) => Some((stream, "127.0.0.1")),
                Err(_) => v6.await.ok().map(|stream| (stream, "::1")),
            },
            Either::Right((result, v4)) => match result {
                Ok(stream) => Some((stream, "::1")),
                Err(_) => v4.await.ok().map(|stream| (stream, "127.0.0.1")),
            },
        }
    })
    .await
    .ok()
    .flatten();

    let Some((stream, host)) = chosen else {
        bail!("локальный сервис на порту {port} недоступен");
    };
    if let Ok(mut map) = preferred_addr().lock() {
        map.insert(port, host);
    }
    Ok(stream)
}

/// На macOS порты 80/443 без root недоступны. Если занять их не удалось,
/// автоматически переключаемся на 8088/8443, чтобы плагин работал «из коробки».
pub async fn preflight_ports(state: &Arc<AppState>) {
    #[cfg(target_os = "macos")]
    {
        let settings = state.settings_snapshot();
        let mut http = settings.local_proxy_port;
        let mut https = settings.local_https_port;
        let http_ok = http != state.panel_port && port_available(http);
        let https_ok = https != state.panel_port && port_available(https);
        if !http_ok {
            if let Some(port) = free_port(8088, state.panel_port) {
                http = port;
            }
        }
        if !https_ok {
            if let Some(port) = free_port(8443, state.panel_port) {
                https = port;
            }
        }
        if http != settings.local_proxy_port || https != settings.local_https_port {
            if let Ok(mut current) = state.settings.lock() {
                current.local_proxy_port = http;
                current.local_https_port = https;
                let _ = current.save(&state.dir);
            }
            wlog!("локальные домены: macOS-порты переключены на HTTP {http}, HTTPS {https}");
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = state;
    }
}

#[cfg(target_os = "macos")]
fn port_available(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
}

#[cfg(target_os = "macos")]
fn free_port(start: u16, exclude: u16) -> Option<u16> {
    (start..=start.saturating_add(100)).find(|port| *port != exclude && port_available(*port))
}

pub async fn serve(state: Arc<AppState>, stop: tokio::sync::watch::Receiver<bool>) -> Result<()> {
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
    // Если системный прокси уже включён, локальные домены должны быть в его
    // исключениях — иначе браузер отправит их в 8081 и они не откроются.
    if crate::systemproxy::get_system_proxy()
        .map(|proxy| proxy.enabled)
        .unwrap_or(false)
    {
        if let Err(e) = crate::systemproxy::set_system_proxy(true, &domain_names) {
            wlog!("системный прокси: исключения локальных доменов — {e:#}");
        }
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
    let mut http_stop = stop.clone();
    tokio::spawn(async move {
        match bind_retry(http_port).await {
            Ok(listener) => {
                if let Ok(mut err) = http_state.local_http_error.lock() {
                    *err = None;
                }
                wlog!("локальные домены: http://127.0.0.1:{http_port}");
                loop {
                    tokio::select! {
                        _ = http_stop.changed() => break,
                        accepted = listener.accept() => {
                            if let Ok((sock, _)) = accepted {
                                let st = http_state.clone();
                                tokio::spawn(async move {
                                    handle_stream(sock, st).await;
                                });
                            }
                        }
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
    let mut https_stop = stop.clone();
    tokio::spawn(async move {
        let listener = match bind_retry(https_port).await {
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
            tokio::select! {
                _ = https_stop.changed() => break,
                accepted = listener.accept() => {
                    if let Ok((sock, _)) = accepted {
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
            }
        }
    });

    // Ждём сигнал отключения плагина; задачи слушателей завершатся сами.
    let mut wait = stop;
    let _ = wait.changed().await;
    Ok(())
}

async fn bind_retry(port: u16) -> std::io::Result<TcpListener> {
    let mut last = None;
    for _ in 0..10 {
        match TcpListener::bind(("127.0.0.1", port)).await {
            Ok(listener) => return Ok(listener),
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("bind failed")))
}

/// Включает плагин локальных доменов (идемпотентно).
pub fn is_running(state: &AppState) -> bool {
    state
        .local_proxy_stop
        .lock()
        .map(|guard| guard.is_some())
        .unwrap_or(false)
}

pub fn start(state: &Arc<AppState>) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static GENERATION: AtomicU64 = AtomicU64::new(0);
    let Ok(mut guard) = state.local_proxy_stop.lock() else {
        return;
    };
    if guard.is_some() {
        return;
    }
    let generation = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let (tx, rx) = tokio::sync::watch::channel(false);
    *guard = Some((generation, tx));
    drop(guard);
    let st = state.clone();
    tokio::spawn(async move {
        if let Err(e) = serve(st.clone(), rx).await {
            wlog!("локальные домены остановлены: {e}");
        }
        if let Ok(mut guard) = st.local_proxy_stop.lock() {
            // Старый serve не должен стирать sender нового запуска.
            if matches!(&*guard, Some((current, _)) if *current == generation) {
                guard.take();
            }
        }
    });
}

/// Выключает плагин локальных доменов.
pub fn stop(state: &Arc<AppState>) {
    if let Ok(mut guard) = state.local_proxy_stop.lock() {
        if let Some((_, tx)) = guard.take() {
            let _ = tx.send(true);
        }
    }
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
    let version = parts.next().unwrap_or("HTTP/1.1").to_string();

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

    // Защита от петли: домен не должен указывать на сам локальный
    // HTTP/HTTPS-слушатель, иначе каждый запрос создаёт новое соединение.
    let local_ports = state.settings_snapshot();
    if port == local_ports.local_proxy_port || port == local_ports.local_https_port {
        let body = "локальный домен указывает на сам прокси; выберите другой порт";
        let response = format!(
            "HTTP/1.1 502 Bad Gateway\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.shutdown().await;
        return;
    }

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

    // Прозрачно пробрасываем запрос как есть: сохраняем Connection/keep-alive,
    // Transfer-Encoding и Upgrade. После первого запроса copy_bidirectional
    // держит соединение — браузер переиспользует TLS и backend keep-alive.
    let mut out = format!("{method} {target} {version}\r\n");
    let mut has_host = false;
    for (k, v) in &headers {
        if k.eq_ignore_ascii_case("proxy-connection") {
            continue;
        }
        if k.eq_ignore_ascii_case("host") {
            has_host = true;
        }
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    if !has_host {
        out.push_str(&format!("Host: {host}\r\n"));
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
