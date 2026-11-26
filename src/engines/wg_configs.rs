//! Несколько WireGuard-конфигов: хранение, выбор активного, live-проверка
//! трафика через временный wireproxy.
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde::Serialize;
use tokio::net::TcpStream;

use crate::engines::wireproxy;
use crate::state::AppState;
use crate::wlog;

const TEST_HTTP_PORT: u16 = 26250;
const TEST_SOCKS_PORT: u16 = 26249;
const TEST_PROBE_MS: u64 = 8000;

static TESTING: AtomicBool = AtomicBool::new(false);

pub fn test_busy() -> bool {
    TESTING.load(Ordering::SeqCst)
}

struct TestGuard;

impl Drop for TestGuard {
    fn drop(&mut self) {
        TESTING.store(false, Ordering::SeqCst);
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WgConfigInfo {
    pub slug: String,
    pub name: String,
    pub endpoint: Option<String>,
    pub active: bool,
    pub valid: bool,
}

fn slugify(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if (ch.is_whitespace() || ch == '-' || ch == '_') && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        format!("wg-{}", chrono::Utc::now().timestamp())
    } else {
        out
    }
}

fn validate_slug(slug: &str) -> Result<()> {
    if slug.is_empty()
        || slug.len() > 120
        || !slug
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        || slug == "."
        || slug == ".."
    {
        bail!("некорректное имя конфига");
    }
    Ok(())
}

pub fn list(store: &crate::store::Store, _dir: &Path) -> Result<Vec<WgConfigInfo>> {
    let mut out: Vec<WgConfigInfo> = store
        .wg_profiles()?
        .into_iter()
        .map(|(slug, name, config, active)| WgConfigInfo {
            slug,
            name,
            endpoint: wireproxy::endpoint_from_text(&config),
            active,
            valid: wireproxy::validate(&config).is_ok(),
        })
        .collect();
    out.sort_by(|a, b| {
        b.active
            .cmp(&a.active)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

pub fn add(store: &crate::store::Store, dir: &Path, name: &str, content: &str) -> Result<String> {
    wireproxy::validate(content)?;
    let slug = slugify(name);
    let was_active = store
        .wg_profiles()
        .unwrap_or_default()
        .iter()
        .any(|(existing, _, _, active)| existing == &slug && *active);
    let first = store.insert_wg_profile(&slug, name.trim(), content)?;
    if first || was_active {
        activate(store, dir, &slug)?;
    }
    Ok(slug)
}

pub fn activate(store: &crate::store::Store, dir: &Path, slug: &str) -> Result<()> {
    validate_slug(slug)?;
    let content = store
        .wg_profile(slug)?
        .with_context(|| format!("конфиг {slug} не найден"))?;
    wireproxy::validate(&content)?;
    let wg = crate::paths::in_dir(dir, "wg.conf")?;
    let tmp = wg.with_extension("conf.tmp");
    std::fs::write(&tmp, content)?;
    if wg.exists() {
        let _ = std::fs::remove_file(&wg);
    }
    std::fs::rename(tmp, &wg)?;
    store.activate_wg_profile(slug)?;
    wireproxy::ensure_config(dir)?;
    Ok(())
}

pub fn remove(
    store: &crate::store::Store,
    dir: &Path,
    slug: &str,
) -> Result<(bool, Option<String>)> {
    validate_slug(slug)?;
    let (was_active, next) = store.delete_wg_profile(slug)?;
    if was_active {
        match next.as_deref() {
            Some(next_slug) => activate(store, dir, next_slug)?,
            None => {
                let _ = std::fs::remove_file(dir.join("wg.conf"));
                let _ = std::fs::remove_file(dir.join("wireproxy.conf"));
            }
        }
    }
    Ok((was_active, next))
}

fn test_config(source: &Path) -> String {
    let abs = std::path::absolute(source).unwrap_or_else(|_| source.to_path_buf());
    format!(
        "WGConfig = {}\n\n[Socks5]\nBindAddress = 127.0.0.1:{TEST_SOCKS_PORT}\n\n[http]\nBindAddress = 127.0.0.1:{TEST_HTTP_PORT}\n",
        abs.display()
    )
}

#[cfg(windows)]
fn cmd_test() -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new(".\\wireproxy.exe");
    cmd.args(["-c", "wg-test.conf"]);
    cmd.creation_flags(0x08000000);
    cmd
}

#[cfg(not(windows))]
fn cmd_test() -> tokio::process::Command {
    let mut cmd = tokio::process::Command::new("./wireproxy");
    cmd.args(["-c", "wg-test.conf"]);
    cmd
}

async fn wait_port(port: u16, deadline: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < deadline {
        if TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    false
}

async fn probe_port(port: u16) -> Option<(u64, String)> {
    let client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(format!("http://127.0.0.1:{port}")).ok()?)
        .timeout(Duration::from_millis(TEST_PROBE_MS))
        .pool_max_idle_per_host(0)
        .build()
        .ok()?;
    let started = Instant::now();
    let resp = client.get("https://api.ipify.org").send().await.ok()?;
    let ip = resp.text().await.ok()?.trim().to_string();
    if ip.is_empty() {
        None
    } else {
        Some((started.elapsed().as_millis() as u64, ip))
    }
}

async fn probe() -> Option<(u64, String)> {
    probe_port(TEST_HTTP_PORT).await
}

/// Поднимает временный wireproxy на этом конфиге и проверяет реальный трафик.
pub async fn test(store: &crate::store::Store, dir: &Path, slug: &str) -> Result<(u64, String)> {
    if TESTING.swap(true, Ordering::SeqCst) {
        bail!("проверка WireGuard уже идёт");
    }
    let _guard = TestGuard;
    if !dir
        .join(if cfg!(windows) {
            "wireproxy.exe"
        } else {
            "wireproxy"
        })
        .is_file()
    {
        bail!("wireproxy не найден в каталоге данных");
    }
    let content = store
        .wg_profile(slug)?
        .with_context(|| format!("конфиг {slug} не найден"))?;
    let source_path = dir.join("wg-test-source.conf");
    std::fs::write(&source_path, content)?;
    let conf = test_config(&source_path);
    let test_path = dir.join("wg-test.conf");
    std::fs::write(&test_path, conf)?;
    let mut child = match cmd_test()
        .current_dir(dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            let _ = std::fs::remove_file(&test_path);
            let _ = std::fs::remove_file(&source_path);
            return Err(error).context("не удалось запустить wireproxy для теста");
        }
    };
    let result = if wait_port(TEST_HTTP_PORT, Duration::from_secs(8)).await {
        probe().await
    } else {
        None
    };
    let _ = child.kill().await;
    let _ = child.wait().await;
    let _ = std::fs::remove_file(&test_path);
    let _ = std::fs::remove_file(&source_path);
    match result {
        Some(value) => Ok(value),
        None => bail!("трафик через конфиг не прошёл"),
    }
}

/// Проверяет все сохранённые WireGuard-конфиги и активирует самый быстрый.
/// Возвращает true, если активный конфиг изменился.
pub async fn select_fastest(state: &Arc<AppState>) -> Result<bool> {
    let settings = state.settings_snapshot();
    if !settings.wireguard_enabled {
        return Ok(false);
    }
    if test_busy() {
        return Ok(false);
    }
    let configs = list(&state.store, &state.dir)?;
    let valid: Vec<WgConfigInfo> = configs.into_iter().filter(|item| item.valid).collect();
    if valid.is_empty() {
        return Ok(false);
    }
    let active = valid.iter().find(|item| item.active).cloned();
    let inactive: Vec<WgConfigInfo> = valid
        .iter()
        .filter(|item| active.as_ref().map(|a| a.slug.as_str()) != Some(item.slug.as_str()))
        .cloned()
        .collect();
    if inactive.is_empty() {
        return Ok(false);
    }
    if valid.len() == 1 {
        let only = &valid[0];
        if only.active {
            return Ok(false);
        }
        if !state.settings_snapshot().auto_rotate {
            return Ok(false);
        }
        activate(&state.store, &state.dir, &only.slug)?;
        if settings.wireguard_enabled {
            state.wg.restart();
        }
        log_active(&state.store, &state.dir, &only.slug);
        return Ok(true);
    }

    // Активный конфиг не поднимаем вторым wireproxy с тем же ключом:
    // измеряем его текущую задержку через уже работающий HTTP-порт.
    let active_ms = if active.is_some() {
        probe_port(crate::vless::WIREPROXY_HTTP_PORT)
            .await
            .map(|(ms, _)| ms)
    } else {
        None
    };
    if let (Some(item), Some(ms)) = (active.as_ref(), active_ms) {
        wlog!("авто-конфиг WireGuard: активный «{}» — {ms} мс", item.name);
    }

    let mut best: Option<(String, String, u64, String)> = None;
    for item in &inactive {
        match test(&state.store, &state.dir, &item.slug).await {
            Ok((ms, ip)) => {
                wlog!("авто-конфиг WireGuard: «{}» — {ms} мс, IP {ip}", item.name);
                if best
                    .as_ref()
                    .map(|(_, _, best_ms, _)| ms < *best_ms)
                    .unwrap_or(true)
                {
                    best = Some((item.slug.clone(), item.name.clone(), ms, ip));
                }
            }
            Err(e) => wlog!("авто-конфиг WireGuard: «{}» — {e:#}", item.name),
        }
    }
    let Some((slug, name, ms, ip)) = best else {
        bail!("ни один запасной WireGuard-конфиг не прошёл проверку трафика");
    };
    if let Some(active_ms) = active_ms {
        if ms >= active_ms {
            if let Some(item) = active.as_ref() {
                wlog!(
                    "авто-конфиг WireGuard: активный «{}» быстрее запасных ({active_ms} мс)",
                    item.name
                );
            }
            return Ok(false);
        }
    }
    if !state.settings_snapshot().auto_rotate {
        return Ok(false);
    }
    activate(&state.store, &state.dir, &slug)?;
    if settings.wireguard_enabled {
        state.wg.restart();
    }
    log_active(&state.store, &state.dir, &slug);
    wlog!("авто-конфиг WireGuard: выбран «{name}» — {ms} мс, IP {ip}");
    Ok(true)
}

/// Небольшая обёртка для логов API.
pub fn log_active(store: &crate::store::Store, dir: &Path, slug: &str) {
    if let Ok(list) = list(store, dir) {
        if let Some(item) = list.iter().find(|item| item.slug == slug) {
            wlog!(
                "WireGuard: активен конфиг «{}» ({:?})",
                item.name,
                item.endpoint
            );
        }
    }
}

/// Writes the active profile to `wg.conf` and `wireproxy.conf` without
/// touching the database. Used on startup after restoring the DB.
pub fn materialize_active(store: &crate::store::Store, dir: &Path) -> Result<()> {
    match store.active_wg_profile()? {
        Some((_slug, content)) => {
            if let Err(error) = wireproxy::validate(&content) {
                crate::wlog!("WireGuard: активный профиль некорректен — {error:#}");
                let _ = std::fs::remove_file(dir.join("wg.conf"));
                let _ = std::fs::remove_file(dir.join("wireproxy.conf"));
                return Ok(());
            }
            let wg = crate::paths::in_dir(dir, "wg.conf")?;
            write_atomic(&wg, &content)?;
            wireproxy::ensure_config(dir)?;
        }
        None => {
            let _ = std::fs::remove_file(dir.join("wg.conf"));
            let _ = std::fs::remove_file(dir.join("wireproxy.conf"));
        }
    }
    Ok(())
}

fn write_atomic(path: &Path, content: &str) -> Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, content)?;
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    std::fs::rename(tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "wpc-wgcfg-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn config(host: &str) -> String {
        format!(
            "[Interface]\nPrivateKey = abc\nAddress = 10.0.0.2/32\n\
             [Peer]\nPublicKey = def\nEndpoint = {host}:51820\n"
        )
    }

    #[test]
    fn overwriting_active_config_updates_wg_conf() {
        let dir = temp();
        let store = crate::store::Store::open(&dir).unwrap();
        add(&store, &dir, "A", &config("1.2.3.4")).unwrap();
        add(&store, &dir, "B", &config("5.6.7.8")).unwrap();
        activate(&store, &dir, "a").unwrap();
        add(&store, &dir, "A", &config("9.9.9.9")).unwrap();
        let wg = std::fs::read_to_string(dir.join("wg.conf")).unwrap();
        assert!(wg.contains("9.9.9.9:51820"), "wg.conf: {wg}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
