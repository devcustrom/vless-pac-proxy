//! Пул конфигов из подписок: безопасная загрузка, разбор vless/ss,
//! проверка `xray -test`, мульти-inbound тест с реальными пробами, применение
//! и авто-ротация. Паритет с legacy/vless-pool.js.
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use reqwest::redirect::Policy;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::process::Command;

use crate::engines::{find_engine, EngineKind};
use crate::net::dns_guard;
use crate::state::AppState;
use crate::vless::link;
use crate::wlog;

pub const MAX_TEST: usize = 120;
const TEST_BASE_PORT: u16 = 26100;
const PROBE_TIMEOUT_MS: u64 = 8000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolEntry {
    pub link: String,
    pub name: String,
    pub protocol: String,
    pub address: String,
    pub port: u16,
    pub ok: bool,
    #[serde(default)]
    pub ms: Option<u64>,
    #[serde(default)]
    pub ip: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct PoolStateFile {
    #[serde(rename = "updatedAt", default)]
    pub updated_at: Option<String>,
    #[serde(rename = "activeLink", default)]
    pub active_link: Option<String>,
    #[serde(default)]
    pub pool: Vec<PoolEntry>,
}

fn xray_dir() -> Result<PathBuf> {
    find_engine(EngineKind::Xray)
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .context("xray не найден рядом с приложением")
}

#[cfg(windows)]
fn cmd_test() -> Command {
    let mut c = Command::new(".\\xray.exe");
    c.args(["-test", "-c", "xray-pool-one.json"]);
    c.creation_flags(0x08000000); // CREATE_NO_WINDOW: без мигающих консолей
    c
}

#[cfg(not(windows))]
fn cmd_test() -> Command {
    let mut c = Command::new("./xray");
    c.args(["-test", "-c", "xray-pool-one.json"]);
    c
}

#[cfg(windows)]
fn cmd_run() -> Command {
    let mut c = Command::new(".\\xray.exe");
    c.args(["-c", "xray-pool-test.json"]);
    c.creation_flags(0x08000000);
    c
}

#[cfg(not(windows))]
fn cmd_run() -> Command {
    let mut c = Command::new("./xray");
    c.args(["-c", "xray-pool-test.json"]);
    c
}

// ---------------- безопасная загрузка ----------------

async fn validate_url(raw: &str) -> Result<url::Url> {
    let u = url::Url::parse(raw).context("некорректный URL")?;
    if u.scheme() != "http" && u.scheme() != "https" {
        bail!("разрешены только http/https");
    }
    let host = u.host_str().context("нет хоста")?;
    dns_guard::check_host(host).await?;
    Ok(u)
}

pub async fn validate_subscription_url(raw: &str) -> Result<()> {
    validate_url(raw).await.map(|_| ())
}

fn no_redirect_client(proxy_port: Option<u16>) -> Result<reqwest::Client> {
    let mut b = reqwest::Client::builder()
        .redirect(Policy::none())
        .timeout(Duration::from_secs(30))
        .user_agent("wireproxy-control/1.0");
    if let Some(port) = proxy_port {
        b = b.proxy(reqwest::Proxy::all(format!("http://127.0.0.1:{port}"))?);
    }
    Ok(b.build()?)
}

async fn fetch_once(client: &reqwest::Client, url: &str) -> Result<FetchOutcome> {
    let resp = client.get(url).send().await?;
    let status = resp.status();
    if status.is_redirection() {
        let loc = resp
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
            .context("редирект без location")?
            .to_string();
        return Ok(FetchOutcome::Redirect(loc));
    }
    if !status.is_success() {
        bail!("HTTP {status}");
    }
    if let Some(len) = resp.content_length() {
        if len > 5 * 1024 * 1024 {
            bail!("ответ слишком большой");
        }
    }
    let text = resp.text().await?;
    if text.len() > 5 * 1024 * 1024 {
        bail!("ответ слишком большой");
    }
    Ok(FetchOutcome::Text(text))
}

enum FetchOutcome {
    Text(String),
    Redirect(String),
}

pub async fn safe_fetch_text(_state: &Arc<AppState>, raw: &str) -> Result<String> {
    let direct = no_redirect_client(None)?;
    let proxied = no_redirect_client(Some(crate::vless::WIREPROXY_HTTP_PORT)).ok();
    let mut current = validate_url(raw).await?;
    for _ in 0..=4 {
        let outcome = match fetch_once(&direct, current.as_str()).await {
            Ok(o) => o,
            Err(first) => match &proxied {
                Some(p) => fetch_once(p, current.as_str())
                    .await
                    .map_err(|e| anyhow::anyhow!("{first}; через прокси: {e}"))?,
                None => return Err(first),
            },
        };
        match outcome {
            FetchOutcome::Text(text) => return Ok(text),
            FetchOutcome::Redirect(loc) => {
                let next = current.join(&loc).context("некорректный редирект")?;
                current = validate_url(next.as_str()).await?;
            }
        }
    }
    bail!("слишком много редиректов")
}

// ---------------- разбор подписки ----------------

pub fn parse_subscription_links(text: &str) -> Vec<String> {
    let mut body = text.to_string();
    if !body.contains("://") {
        use base64::Engine;
        let cleaned: String = body.chars().filter(|c| !c.is_whitespace()).collect();
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(cleaned.as_bytes()) {
            if let Ok(decoded) = String::from_utf8(bytes) {
                body = decoded;
            }
        }
    }
    let mut out = Vec::new();
    for line in body.lines() {
        let s = line.trim();
        if s.is_empty() || s.starts_with('#') {
            continue;
        }
        let lower = s.to_ascii_lowercase();
        if lower.starts_with("vless://") || lower.starts_with("ss://") {
            out.push(s.to_string());
        }
    }
    out
}

fn round_robin(sources: Vec<Vec<String>>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut idx = 0usize;
    loop {
        let mut added = false;
        for src in &sources {
            if let Some(link) = src.get(idx) {
                added = true;
                if seen.insert(link.clone()) {
                    out.push(link.clone());
                }
            }
        }
        if !added {
            break;
        }
        idx += 1;
    }
    out
}

// ---------------- проверка и тестирование ----------------

struct Candidate {
    link: String,
    parsed: crate::vless::link::ParsedLink,
}

async fn validate_one(dir: &PathBuf, candidate: &Candidate) -> Result<bool> {
    let (config, _) = link::generate_config(&candidate.link)?;
    let one = dir.join("xray-pool-one.json");
    std::fs::write(&one, serde_json::to_string(&config)?)?;
    let status = cmd_test()
        .current_dir(dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .context("xray -test не запустился")?;
    let ok = status.success();
    let _ = std::fs::remove_file(&one);
    Ok(ok)
}

async fn wait_port(ports: &[u16], deadline: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < deadline {
        for port in ports {
            if TcpStream::connect(("127.0.0.1", *port)).await.is_ok() {
                return true;
            }
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    false
}

async fn probe_via_port(port: u16) -> Option<(u64, String)> {
    let client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(format!("http://127.0.0.1:{port}")).ok()?)
        .timeout(Duration::from_millis(PROBE_TIMEOUT_MS))
        .build()
        .ok()?;
    let started = Instant::now();
    let resp = client.get("https://api.ipify.org").send().await.ok()?;
    let ip = resp.text().await.ok()?;
    let ip = ip.trim().to_string();
    if ip.is_empty() {
        None
    } else {
        Some((started.elapsed().as_millis() as u64, ip))
    }
}

async fn test_links(dir: &PathBuf, links: Vec<String>) -> Vec<PoolEntry> {
    // 1) per-link xray -test: битые конфиги не роняют общий тест
    let mut prepared: Vec<Candidate> = Vec::new();
    for link in links.into_iter().take(MAX_TEST) {
        let Ok(parsed) = link::parse_any(&link) else {
            continue;
        };
        let candidate = Candidate { link, parsed };
        if validate_one(dir, &candidate).await.unwrap_or(false) {
            prepared.push(candidate);
        }
    }
    if prepared.is_empty() {
        return Vec::new();
    }

    // 2) один тестовый xray: по inbound на каждый конфиг
    let mut inbounds = Vec::new();
    let mut outbounds = Vec::new();
    let mut rules = Vec::new();
    for (i, cand) in prepared.iter().enumerate() {
        inbounds.push(serde_json::json!({
            "listen": "127.0.0.1",
            "port": TEST_BASE_PORT + i as u16,
            "protocol": "http",
            "tag": format!("in{i}")
        }));
        let outbound = link::build_outbound(&cand.parsed, &format!("out{i}"));
        outbounds.push(outbound);
        rules.push(serde_json::json!({
            "type": "field",
            "inboundTag": [format!("in{i}")],
            "outboundTag": format!("out{i}")
        }));
    }
    let config = serde_json::json!({
        "log": { "loglevel": "none" },
        "inbounds": inbounds,
        "outbounds": outbounds,
        "routing": { "rules": rules }
    });
    let test_cfg = dir.join("xray-pool-test.json");
    if std::fs::write(
        &test_cfg,
        serde_json::to_string(&config).unwrap_or_default(),
    )
    .is_err()
    {
        return Vec::new();
    }
    let mut child = match cmd_run()
        .current_dir(dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            wlog!("пул: не удалось запустить тестовый xray — {e}");
            let _ = std::fs::remove_file(&test_cfg);
            return Vec::new();
        }
    };
    let ports: Vec<u16> = (0..prepared.len())
        .map(|i| TEST_BASE_PORT + i as u16)
        .collect();
    let ready = wait_port(&ports, Duration::from_secs(6)).await;
    let mut results = Vec::new();
    if ready {
        let probes = futures_util::future::join_all(ports.iter().map(|p| probe_via_port(*p))).await;
        for (i, cand) in prepared.iter().enumerate() {
            let probe = probes.get(i).cloned().flatten();
            results.push(PoolEntry {
                link: cand.link.clone(),
                name: cand.parsed.name.clone(),
                protocol: cand.parsed.protocol.clone(),
                address: cand.parsed.address.clone(),
                port: cand.parsed.port,
                ok: probe.is_some(),
                ms: probe.as_ref().map(|(ms, _)| *ms),
                ip: probe.map(|(_, ip)| ip),
            });
        }
    } else {
        wlog!("пул: тестовый xray не поднял порты");
    }
    let _ = child.kill().await;
    let _ = std::fs::remove_file(&test_cfg);
    results
}

// ---------------- обновление пула ----------------

pub async fn refresh(state: &Arc<AppState>) -> Result<()> {
    {
        let mut pool = state.pool.lock().unwrap();
        if pool.testing {
            bail!("проверка уже идёт");
        }
        pool.testing = true;
        pool.error = None;
        pool.last_attempt_at = Some(chrono::Utc::now());
    }
    let result = refresh_inner(state).await;
    {
        let mut pool = state.pool.lock().unwrap();
        pool.testing = false;
        if let Err(e) = &result {
            pool.error = Some(e.to_string());
        }
    }
    result
}

async fn refresh_inner(state: &Arc<AppState>) -> Result<()> {
    let subs = read_subscriptions(&state.dir);
    if subs.is_empty() {
        bail!("нет подписок: добавьте URL");
    }
    let mut sources: Vec<Vec<String>> = Vec::new();
    for url in &subs {
        match safe_fetch_text(state, url).await {
            Ok(text) => {
                let links = parse_subscription_links(&text);
                wlog!("подписка {}: {} конфигов", host_of(url), links.len());
                sources.push(links);
            }
            Err(e) => wlog!("подписка {}: ошибка — {e}", host_of(url)),
        }
    }
    let links = round_robin(sources);
    if links.is_empty() {
        bail!("в подписках не найдено поддерживаемых конфигов (vless://, ss://)");
    }
    wlog!(
        "пул: проверяю {} из {} конфигов...",
        links.len().min(MAX_TEST),
        links.len()
    );
    let dir = xray_dir()?;
    let mut entries = test_links(&dir, links).await;
    entries.sort_by(|a, b| {
        b.ok.cmp(&a.ok)
            .then(a.ms.unwrap_or(u64::MAX).cmp(&b.ms.unwrap_or(u64::MAX)))
    });
    let ok_count = entries.iter().filter(|e| e.ok).count();
    {
        let mut pool = state.pool.lock().unwrap();
        pool.entries = entries;
        pool.updated_at = Some(chrono::Utc::now());
        pool.tried.clear();
        pool.error = None;
    }
    save_pool_state(state);
    wlog!("пул обновлён: рабочих {ok_count}");
    Ok(())
}

fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|s| s.to_string()))
        .unwrap_or_else(|| url.to_string())
}

pub fn read_subscriptions(dir: &std::path::Path) -> Vec<String> {
    let Ok(path) = crate::paths::in_dir(dir, "subscriptions.txt") else {
        return Vec::new();
    };
    std::fs::read_to_string(path)
        .map(|t| {
            t.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .collect()
        })
        .unwrap_or_default()
}

pub fn write_subscriptions(dir: &std::path::Path, list: &[String]) -> std::io::Result<()> {
    let path = crate::paths::in_dir(dir, "subscriptions.txt")
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()))?;
    let mut text = String::from("# Подписки-источники конфигов (по одному URL в строке).\n\n");
    let mut seen = HashSet::new();
    for u in list {
        if seen.insert(u) {
            text.push_str(u);
            text.push('\n');
        }
    }
    std::fs::write(path, text)
}

// ---------------- пул: сохранение и применение ----------------

pub fn save_pool_state(state: &Arc<AppState>) {
    let (entries, updated, active) = {
        let pool = state.pool.lock().unwrap();
        (
            pool.entries.clone(),
            pool.updated_at.map(|t| t.to_rfc3339()),
            pool.active_link.clone(),
        )
    };
    let file = PoolStateFile {
        updated_at: updated,
        active_link: active,
        pool: entries,
    };
    if let Ok(path) = crate::paths::in_dir(&state.dir, "pool-state.json") {
        if let Ok(text) = serde_json::to_string_pretty(&file) {
            let _ = std::fs::write(path, text);
        }
    }
}

pub fn load_pool_state(state: &Arc<AppState>) {
    let Ok(path) = crate::paths::in_dir(&state.dir, "pool-state.json") else {
        return;
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(file) = serde_json::from_str::<PoolStateFile>(&text) else {
        return;
    };
    let mut pool = state.pool.lock().unwrap();
    pool.entries = file.pool;
    pool.updated_at = file
        .updated_at
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(&s).ok())
        .map(|d| d.with_timezone(&chrono::Utc));
    pool.active_link = file.active_link;
}

pub fn apply_entry(state: &Arc<AppState>, index: usize) -> Result<link::LinkInfo> {
    let (entry, _link) = {
        let pool = state.pool.lock().unwrap();
        let Some(entry) = pool.entries.get(index) else {
            bail!("нет такого конфига в пуле");
        };
        (entry.clone(), entry.link.clone())
    };
    let info = link::save_link(&state.dir, &entry.link)?;
    {
        let mut pool = state.pool.lock().unwrap();
        pool.active_link = Some(entry.link.clone());
        pool.last_rotate_at = Some(chrono::Utc::now());
    }
    state.xr.restart();
    save_pool_state(state);
    wlog!(
        "применён конфиг из пула: {} ({} {}:{})",
        entry.name,
        info.protocol,
        info.address,
        info.port
    );
    Ok(info)
}

/// Автовосстановление: при мёртвом VLESS применить рабочий конфиг из пула.
/// Не чаще раза в минуту; после исчерпания кандидатов — перекачать подписки.
pub async fn maybe_auto_recover(state: &Arc<AppState>, reason: &str) -> bool {
    let settings = state.settings_snapshot();
    if !settings.auto_rotate {
        return false;
    }
    let now = chrono::Utc::now();
    {
        let pool = state.pool.lock().unwrap();
        if pool.testing {
            return false;
        }
        if let Some(last) = pool.last_rotate_at {
            if (now - last).num_seconds() < 60 {
                return false;
            }
        }
    }
    // пул устарел или пуст — обновляем
    let stale = {
        let pool = state.pool.lock().unwrap();
        let no_ok = !pool.entries.iter().any(|e| e.ok);
        let stale = match pool.updated_at {
            Some(t) => (now - t).num_seconds() > (settings.auto_pool_refresh_hours * 3600.0) as i64,
            None => true,
        };
        let attempt_ok = match pool.last_attempt_at {
            Some(t) => (now - t).num_seconds() > 90,
            None => true,
        };
        (no_ok || stale) && attempt_ok
    };
    if stale {
        wlog!("автовосстановление ({reason}): пул устарел/пуст — обновляю подписки");
        let _ = refresh(state).await;
    }
    let candidate = {
        let mut pool = state.pool.lock().unwrap();
        if let Some(cycle) = pool.rotate_cycle_at {
            if (now - cycle).num_seconds() > 900 {
                pool.tried.clear();
                pool.rotate_cycle_at = Some(now);
            }
        } else {
            pool.rotate_cycle_at = Some(now);
        }
        let active = pool.active_link.clone();
        let idx = pool.entries.iter().position(|e| {
            e.ok && Some(&e.link) != active.as_ref() && !pool.tried.contains(&e.link)
        });
        idx.map(|i| {
            let entry = pool.entries[i].clone();
            pool.tried.insert(entry.link.clone());
            entry
        })
    };
    let Some(entry) = candidate else {
        let mut pool = state.pool.lock().unwrap();
        if !pool.tried.is_empty() {
            pool.tried.clear();
            wlog!("автовосстановление: все рабочие кандидаты испробованы");
        }
        return false;
    };
    match link::save_link(&state.dir, &entry.link) {
        Ok(info) => {
            let mut pool = state.pool.lock().unwrap();
            pool.active_link = Some(entry.link.clone());
            pool.last_rotate_at = Some(now);
            drop(pool);
            state.xr.restart();
            save_pool_state(state);
            wlog!(
                "автовосстановление ({reason}): VLESS → {} ({} {}:{}, {} мс)",
                entry.name,
                info.protocol,
                info.address,
                info.port,
                entry.ms.unwrap_or(0)
            );
            true
        }
        Err(e) => {
            wlog!("автовосстановление: не удалось применить конфиг — {e}");
            false
        }
    }
}

/// Плановое автообновление подписок (при старте и по расписанию).
pub fn spawn_scheduler(state: Arc<AppState>) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(40)).await;
        loop {
            let settings = state.settings_snapshot();
            if settings.auto_pool_refresh_hours > 0.0 {
                let now = chrono::Utc::now();
                let due = {
                    let pool = state.pool.lock().unwrap();
                    match pool.last_attempt_at {
                        Some(t) => {
                            (now - t).num_seconds()
                                > (settings.auto_pool_refresh_hours * 3600.0) as i64
                        }
                        None => true,
                    }
                };
                if due {
                    if let Err(e) = refresh(&state).await {
                        wlog!("плановое обновление пула: {e}");
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(10 * 60)).await;
        }
    });
}

#[allow(dead_code)]
async fn drain(_s: &mut TcpStream) -> std::io::Result<usize> {
    let mut buf = [0u8; 1024];
    _s.read(&mut buf).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_and_base64_subs() {
        let plain = "# c\nvless://a@b:1?x=1#n\nss://YWVzOnA@1.2.3.4:1#x\nvmess://skip\n";
        let links = parse_subscription_links(plain);
        assert_eq!(links.len(), 2);
        assert!(links[0].starts_with("vless://"));

        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode("vless://a@b:1#n\n".as_bytes());
        let links = parse_subscription_links(&format!("{b64}\n"));
        assert_eq!(links.len(), 1);
    }

    #[test]
    fn round_robin_dedupes() {
        let out = round_robin(vec![
            vec!["a".into(), "b".into()],
            vec!["b".into(), "c".into()],
        ]);
        assert_eq!(out, vec!["a", "b", "c"]);
    }
}
