//! Несколько WireGuard-конфигов: хранение, выбор активного, live-проверка
//! трафика через временный wireproxy.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use tokio::net::TcpStream;

use crate::engines::wireproxy;
use crate::wlog;

const TEST_HTTP_PORT: u16 = 26250;
const TEST_SOCKS_PORT: u16 = 26249;
const TEST_PROBE_MS: u64 = 8000;

static TESTING: AtomicBool = AtomicBool::new(false);

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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Meta {
    #[serde(default)]
    active: Option<String>,
    #[serde(default)]
    names: HashMap<String, String>,
}

fn configs_dir(dir: &Path) -> PathBuf {
    dir.join("wg-configs")
}

fn meta_path(dir: &Path) -> PathBuf {
    dir.join("wg-configs.json")
}

fn read_meta(dir: &Path) -> Meta {
    std::fs::read_to_string(meta_path(dir))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_meta(dir: &Path, meta: &Meta) -> Result<()> {
    let text = serde_json::to_string_pretty(meta)?;
    let tmp = meta_path(dir).with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, meta_path(dir))?;
    Ok(())
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

fn config_path(dir: &Path, slug: &str) -> Result<PathBuf> {
    if slug.is_empty() || slug.contains(['/', '\\']) || slug == "." || slug == ".." {
        bail!("некорректное имя конфига");
    }
    Ok(configs_dir(dir).join(format!("{slug}.conf")))
}

/// Импортирует текущий wg.conf в список конфигов. Импорт не зависит от
/// строгой валидности: конфиг должен быть виден пользователю даже с ошибкой.
pub fn ensure_imported(dir: &Path) -> Result<()> {
    let base = configs_dir(dir);
    std::fs::create_dir_all(&base)?;
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    for entry in std::fs::read_dir(&base)
        .with_context(|| format!("не удалось прочитать каталог {}", base.display()))?
    {
        let path = entry?.path();
        if path.extension().map(|ext| ext == "conf").unwrap_or(false) {
            files.push(path);
        }
    }
    let wg_content = std::fs::read_to_string(dir.join("wg.conf"))
        .ok()
        .filter(|text| !text.trim().is_empty());
    let mut meta = read_meta(dir);

    if files.is_empty() {
        if let Some(content) = wg_content {
            std::fs::write(base.join("default.conf"), content)?;
            meta.active = Some("default".to_string());
            meta.names
                .entry("default".to_string())
                .or_insert_with(|| "Основной".to_string());
            write_meta(dir, &meta)?;
        }
        return Ok(());
    }

    if let Some(content) = wg_content {
        let represented = files.iter().any(|path| {
            std::fs::read_to_string(path)
                .map(|text| text.trim() == content.trim())
                .unwrap_or(false)
        });
        if !represented {
            std::fs::write(base.join("current.conf"), &content)?;
            meta.active = Some("current".to_string());
            meta.names
                .insert("current".to_string(), "Текущий wg.conf".to_string());
            write_meta(dir, &meta)?;
        }
    }
    if meta.active.is_none() {
        if let Some(path) = files.first() {
            if let Some(slug) = path.file_stem().and_then(|stem| stem.to_str()) {
                meta.active = Some(slug.to_string());
                write_meta(dir, &meta)?;
            }
        }
    }
    Ok(())
}

pub fn list(dir: &Path) -> Result<Vec<WgConfigInfo>> {
    ensure_imported(dir)?;
    let meta = read_meta(dir);
    let mut out = Vec::new();
    let entries = std::fs::read_dir(configs_dir(dir))
        .with_context(|| format!("не удалось прочитать {}", configs_dir(dir).display()))?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map(|ext| ext != "conf").unwrap_or(true) {
            continue;
        }
        let Some(slug) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let name = meta
            .names
            .get(slug)
            .cloned()
            .unwrap_or_else(|| slug.to_string());
        out.push(WgConfigInfo {
            slug: slug.to_string(),
            name,
            endpoint: wireproxy::endpoint_from_text(&text),
            active: meta.active.as_deref() == Some(slug),
            valid: wireproxy::validate(&text).is_ok(),
        });
    }
    out.sort_by(|a, b| {
        b.active
            .cmp(&a.active)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

pub fn add(dir: &Path, name: &str, content: &str) -> Result<String> {
    wireproxy::validate(content)?;
    ensure_imported(dir)?;
    let slug = slugify(name);
    let path = config_path(dir, &slug)?;
    std::fs::write(&path, content)?;
    let mut meta = read_meta(dir);
    meta.names.insert(slug.clone(), name.trim().to_string());
    let first = meta.active.is_none();
    let active = meta.active.as_deref() == Some(slug.as_str());
    if first {
        meta.active = Some(slug.clone());
    }
    write_meta(dir, &meta)?;
    // Перезапись активного конфига должна обновить wg.conf, иначе рестарт
    // wireproxy поднимет старые ключи/peer.
    if first || active {
        activate(dir, &slug)?;
    }
    Ok(slug)
}

pub fn activate(dir: &Path, slug: &str) -> Result<()> {
    let path = config_path(dir, slug)?;
    let content =
        std::fs::read_to_string(&path).with_context(|| format!("конфиг {slug} не найден"))?;
    wireproxy::validate(&content)?;
    let wg = crate::paths::in_dir(dir, "wg.conf")?;
    let tmp = wg.with_extension("conf.tmp");
    std::fs::write(&tmp, content)?;
    if wg.exists() {
        let _ = std::fs::remove_file(&wg);
    }
    std::fs::rename(tmp, &wg)?;
    wireproxy::ensure_config(dir)?;
    let mut meta = read_meta(dir);
    meta.active = Some(slug.to_string());
    write_meta(dir, &meta)?;
    Ok(())
}

pub fn remove(dir: &Path, slug: &str) -> Result<(bool, Option<String>)> {
    let path = config_path(dir, slug)?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    let mut meta = read_meta(dir);
    meta.names.remove(slug);
    let was_active = meta.active.as_deref() == Some(slug);
    if !was_active {
        write_meta(dir, &meta)?;
        return Ok((false, meta.active));
    }
    let next = std::fs::read_dir(configs_dir(dir))
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().map(|ext| ext == "conf").unwrap_or(false))
        .and_then(|path| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_string)
        });
    match next {
        Some(next_slug) => {
            write_meta(dir, &meta)?;
            activate(dir, &next_slug)?;
            Ok((true, Some(next_slug)))
        }
        None => {
            meta.active = None;
            write_meta(dir, &meta)?;
            let _ = std::fs::remove_file(dir.join("wg.conf"));
            let _ = std::fs::remove_file(dir.join("wireproxy.conf"));
            Ok((true, None))
        }
    }
}

fn test_config(dir: &Path, slug: &str) -> Result<String> {
    let path = config_path(dir, slug)?;
    if !path.is_file() {
        bail!("конфиг {slug} не найден");
    }
    let abs = std::path::absolute(&path).unwrap_or(path);
    Ok(format!(
        "WGConfig = {}\n\n[Socks5]\nBindAddress = 127.0.0.1:{TEST_SOCKS_PORT}\n\n[http]\nBindAddress = 127.0.0.1:{TEST_HTTP_PORT}\n",
        abs.display()
    ))
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

async fn probe() -> Option<(u64, String)> {
    let client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(format!("http://127.0.0.1:{TEST_HTTP_PORT}")).ok()?)
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

/// Поднимает временный wireproxy на этом конфиге и проверяет реальный трафик.
pub async fn test(dir: &Path, slug: &str) -> Result<(u64, String)> {
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
    let conf = test_config(dir, slug)?;
    let test_path = dir.join("wg-test.conf");
    std::fs::write(&test_path, conf)?;
    let mut child = cmd_test()
        .current_dir(dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("не удалось запустить wireproxy для теста")?;
    let result = if wait_port(TEST_HTTP_PORT, Duration::from_secs(8)).await {
        probe().await
    } else {
        None
    };
    let _ = child.kill().await;
    let _ = child.wait().await;
    let _ = std::fs::remove_file(&test_path);
    match result {
        Some(value) => Ok(value),
        None => bail!("трафик через конфиг не прошёл"),
    }
}

/// Небольшая обёртка для логов API.
pub fn log_active(dir: &Path, slug: &str) {
    if let Ok(list) = list(dir) {
        if let Some(item) = list.iter().find(|item| item.slug == slug) {
            wlog!(
                "WireGuard: активен конфиг «{}» ({:?})",
                item.name,
                item.endpoint
            );
        }
    }
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
        add(&dir, "A", &config("1.2.3.4")).unwrap();
        add(&dir, "B", &config("5.6.7.8")).unwrap();
        activate(&dir, "a").unwrap();
        add(&dir, "A", &config("9.9.9.9")).unwrap();
        let wg = std::fs::read_to_string(dir.join("wg.conf")).unwrap();
        assert!(wg.contains("9.9.9.9:51820"), "wg.conf: {wg}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
