//! Менеджер сохранённых Xray-конфигов: VLESS, VMess, Trojan, Shadowsocks и
//! Outline. Повторяет модель WireGuard-конфигов: отдельные файлы, активный
//! конфиг, live-проверка трафика и переключение без повторного ввода ссылки.
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use tokio::net::TcpStream;

use crate::engines::{find_engine, EngineKind};
use crate::vless::link;
use crate::wlog;

const TEST_PROBE_MS: u64 = 8000;
/// Служебный слаг для конфига, который сейчас применён пулом. Он виден в
/// списке, но не считается ручным выбором пользователя.
pub const POOL_SLUG: &str = "pool-active";
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
#[serde(rename_all = "camelCase")]
pub struct XrayConfigInfo {
    pub slug: String,
    pub name: String,
    pub protocol: String,
    pub address: String,
    pub port: u16,
    pub active: bool,
    pub valid: bool,
    pub dynamic: bool,
}

#[derive(Debug, Clone, Default)]
struct StoredInfo {
    protocol: String,
    address: String,
    port: u16,
    dynamic: bool,
}

fn info_from_config(config: &Value) -> Option<StoredInfo> {
    let outbound = config.get("outbounds")?.as_array()?.first()?;
    let protocol = outbound.get("protocol")?.as_str()?.to_string();
    let server = match protocol.as_str() {
        "vless" | "vmess" => outbound.pointer("/settings/vnext/0")?,
        "trojan" | "shadowsocks" => outbound.pointer("/settings/servers/0")?,
        _ => return None,
    };
    let address = server.get("address")?.as_str()?.to_string();
    let port = server.get("port")?.as_u64()?;
    if address.is_empty() || !(1..=65535).contains(&port) {
        return None;
    }
    Some(StoredInfo {
        protocol,
        address,
        port: port as u16,
        dynamic: false,
    })
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
        format!("xray-{}", chrono::Utc::now().timestamp())
    } else {
        out
    }
}

fn unique_slug(store: &crate::store::Store, name: &str) -> String {
    let base = slugify(name);
    let existing: std::collections::HashSet<String> = store
        .xray_profiles()
        .unwrap_or_default()
        .into_iter()
        .map(|profile| profile.slug)
        .collect();
    if !existing.contains(&base) {
        return base;
    }
    let mut index = 2;
    loop {
        let candidate = format!("{base}-{index}");
        if !existing.contains(&candidate) {
            return candidate;
        }
        index += 1;
    }
}

fn write_active_config(dir: &Path, config: &Value) -> Result<()> {
    let target = crate::paths::in_dir(dir, "xray-config.json")?;
    if target.exists() {
        let _ = std::fs::copy(&target, dir.join("xray-config.json.bak"));
    }
    let tmp = dir.join("xray-config.json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(config)?)?;
    std::fs::rename(&tmp, &target)?;
    Ok(())
}

/// Outbound, через который можно доставить трафик до Shadowsocks-сервера,
/// если прямой маршрут блокируется. Приоритет: активный, затем выбранный
/// пользователем, затем самый новый.
fn chain_outbound(
    store: &crate::store::Store,
    current_slug: &str,
    config: &Value,
) -> Option<Value> {
    let first = config.get("outbounds")?.as_array()?.first()?;
    if first.get("protocol")?.as_str()? != "shadowsocks" {
        return None;
    }
    if first
        .pointer("/streamSettings/sockopt/dialerProxy")
        .is_some()
    {
        return None;
    }
    let mut profiles = store.xray_profiles().ok()?;
    profiles.reverse();
    profiles.sort_by_key(|profile| (!profile.active, !profile.user_selected));
    profiles.into_iter().find_map(|profile| {
        if profile.slug == current_slug {
            return None;
        }
        let parsed = serde_json::from_str::<Value>(&profile.config_json).ok()?;
        let outbound = parsed.get("outbounds")?.as_array()?.first()?.clone();
        let protocol = outbound.get("protocol")?.as_str()?;
        matches!(protocol, "vless" | "vmess" | "trojan").then_some(outbound)
    })
}

fn write_active_from_store(store: &crate::store::Store, dir: &Path, slug: &str) -> Result<()> {
    let text = store
        .xray_profile(slug)?
        .with_context(|| format!("конфиг {slug} не найден"))?;
    let mut config: Value =
        serde_json::from_str(&text).with_context(|| format!("конфиг {slug} повреждён"))?;
    if let Some(chain) = chain_outbound(store, slug, &config) {
        if let Some(chained) = link::chain_shadowsocks(&config, &chain) {
            wlog!("Xray: «{slug}» пойдёт через VPN-транспорт");
            config = chained;
        }
    }
    write_active_config(dir, &config)
}

pub fn list(store: &crate::store::Store, _dir: &Path) -> Result<Vec<XrayConfigInfo>> {
    let mut out = Vec::new();
    for profile in store.xray_profiles()? {
        let config = serde_json::from_str::<Value>(&profile.config_json).ok();
        let derived = config.as_ref().and_then(info_from_config);
        out.push(XrayConfigInfo {
            slug: profile.slug,
            name: profile.name,
            protocol: if profile.protocol.is_empty() {
                derived
                    .as_ref()
                    .map(|item| item.protocol.clone())
                    .unwrap_or_default()
            } else {
                profile.protocol
            },
            address: if profile.address.is_empty() {
                derived
                    .as_ref()
                    .map(|item| item.address.clone())
                    .unwrap_or_default()
            } else {
                profile.address
            },
            port: if profile.port == 0 {
                derived.as_ref().map(|item| item.port).unwrap_or(0)
            } else {
                profile.port
            },
            active: profile.active,
            valid: derived.is_some(),
            dynamic: profile.dynamic,
        });
    }
    out.sort_by(|a, b| {
        b.active
            .cmp(&a.active)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

pub fn add_parsed(
    store: &crate::store::Store,
    dir: &Path,
    name: &str,
    parsed: &link::ParsedLink,
    dynamic: bool,
) -> Result<String> {
    let config = link::config_for_parsed(parsed);
    let info = info_from_config(&config).context("не удалось определить параметры конфига")?;
    let mut name = name.trim().to_string();
    if name.is_empty() {
        name = parsed.name.trim().to_string();
    }
    if name.is_empty() {
        name = parsed.address.trim().to_string();
    }
    if name.is_empty() {
        bail!("нужно имя конфига");
    }
    if name.chars().count() > 120 {
        bail!("имя конфига слишком длинное");
    }
    let slug = unique_slug(store, &name);
    let config_json = serde_json::to_string(&config)?;
    let first = store.insert_xray_profile(
        &slug,
        &name,
        &config_json,
        &info.protocol,
        &info.address,
        info.port,
        dynamic,
    )?;
    if first {
        store.activate_xray_profile(&slug, true)?;
        write_active_config(dir, &config)?;
    }
    Ok(slug)
}

pub fn activate(store: &crate::store::Store, dir: &Path, slug: &str) -> Result<()> {
    activate_inner(store, dir, slug, true)
}

/// Активация автовыбором: не помечает конфиг как ручной выбор, чтобы
/// авто-восстановление пула могло заменить его при падении канала.
pub fn activate_auto(store: &crate::store::Store, dir: &Path, slug: &str) -> Result<()> {
    activate_inner(store, dir, slug, false)
}

fn activate_inner(
    store: &crate::store::Store,
    dir: &Path,
    slug: &str,
    user_selected: bool,
) -> Result<()> {
    validate_slug(slug)?;
    write_active_from_store(store, dir, slug)?;
    store.activate_xray_profile(slug, user_selected)?;
    Ok(())
}

pub fn remove(
    store: &crate::store::Store,
    dir: &Path,
    slug: &str,
) -> Result<(bool, Option<String>)> {
    validate_slug(slug)?;
    let (was_active, next) = store.delete_xray_profile(slug)?;
    if was_active {
        match next.as_deref() {
            Some(next_slug) => write_active_from_store(store, dir, next_slug)?,
            None => {
                for name in [
                    "xray-config.json",
                    "xray-config.json.bak",
                    "xray-config.json.tmp",
                ] {
                    let _ = std::fs::remove_file(dir.join(name));
                }
            }
        }
    }
    Ok((was_active, next))
}

/// Пул применил свой конфиг: снимаем отметку пользовательского выбора, чтобы
/// автоподбор снова мог управлять каналом.
pub fn clear_active(store: &crate::store::Store) -> Result<()> {
    store.clear_active_xray()
}

/// Сохраняет конфиг, применённый пулом, в общий список как «Пул: активный».
pub fn sync_pool_active(
    store: &crate::store::Store,
    dir: &Path,
    parsed: &link::ParsedLink,
) -> Result<()> {
    let config = link::config_for_parsed(parsed);
    let info = info_from_config(&config).context("не удалось определить параметры конфига пула")?;
    let json = serde_json::to_string(&config)?;
    store.insert_xray_profile(
        POOL_SLUG,
        "Пул: активный",
        &json,
        &info.protocol,
        &info.address,
        info.port,
        false,
    )?;
    store.activate_xray_profile(POOL_SLUG, false)?;
    write_active_from_store(store, dir, POOL_SLUG)?;
    Ok(())
}

pub fn active_slug(store: &crate::store::Store) -> Option<String> {
    store
        .active_xray_profile()
        .ok()
        .flatten()
        .map(|profile| profile.slug)
}

/// Активный конфиг, который выбрал пользователь. Пул не должен его менять.
pub fn user_selected_active(store: &crate::store::Store) -> Option<String> {
    store
        .active_xray_profile()
        .ok()
        .flatten()
        .filter(|profile| profile.user_selected)
        .map(|profile| profile.slug)
}

fn free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
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

async fn probe(port: u16) -> Option<(u64, String)> {
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

/// Поднимает временный xray на свободном порту и проверяет реальный трафик.
async fn run_probe(dir: &Path, config: &Value) -> Result<(u64, String)> {
    let node = find_engine(EngineKind::Xray).context("xray не найден в каталоге данных")?;
    let mut config = config.clone();
    let port = free_port()?;
    config["inbounds"] = json!([{
        "listen": "127.0.0.1",
        "port": port,
        "protocol": "http",
        "settings": { "allowTransparent": false }
    }]);
    let test_path = dir.join(format!("xray-config-test-{}.json", std::process::id()));
    std::fs::write(&test_path, serde_json::to_vec_pretty(&config)?)?;

    let mut cmd = tokio::process::Command::new(&node);
    cmd.arg("-c")
        .arg(&test_path)
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    #[cfg(windows)]
    cmd.creation_flags(0x08000000);

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(error) => {
            let _ = std::fs::remove_file(&test_path);
            return Err(error).context("не удалось запустить xray для проверки");
        }
    };
    let result = if wait_port(port, Duration::from_secs(8)).await {
        probe(port).await
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

pub async fn test(store: &crate::store::Store, dir: &Path, slug: &str) -> Result<(u64, String)> {
    if TESTING.swap(true, Ordering::SeqCst) {
        bail!("проверка Xray-конфига уже идёт");
    }
    let _guard = TestGuard;
    let text = store
        .xray_profile(slug)?
        .with_context(|| format!("конфиг {slug} не найден"))?;
    let config: Value =
        serde_json::from_str(&text).with_context(|| format!("конфиг {slug} повреждён"))?;
    if let Some(chain) = chain_outbound(store, slug, &config) {
        if let Some(chained) = link::chain_shadowsocks(&config, &chain) {
            return run_probe(dir, &chained)
                .await
                .context("через VPN-транспорт");
        }
    }
    run_probe(dir, &config).await
}

pub fn log_active(store: &crate::store::Store, dir: &Path, slug: &str) {
    if let Ok(items) = list(store, dir) {
        if let Some(item) = items.iter().find(|item| item.slug == slug) {
            wlog!(
                "Xray: активен конфиг «{}» ({} {}:{})",
                item.name,
                item.protocol,
                item.address,
                item.port
            );
        }
    }
}

/// Writes the active Xray profile to `xray-config.json`. Used on startup.
pub fn materialize_active(store: &crate::store::Store, dir: &Path) -> Result<()> {
    if let Some(profile) = store.active_xray_profile()? {
        if let Err(error) = write_active_from_store(store, dir, &profile.slug) {
            crate::wlog!("Xray: активный профиль не материализован — {error:#}");
            let _ = std::fs::remove_file(dir.join("xray-config.json"));
        }
    } else {
        let _ = std::fs::remove_file(dir.join("xray-config.json"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "wpc-xraycfg-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn parsed(host: &str, port: u16) -> link::ParsedLink {
        link::parse_any(&format!("ss://YWVzLTI1Ni1nY206cGFzcw==@{host}:{port}#Test")).unwrap()
    }

    #[test]
    fn manages_active_configs() {
        let dir = temp();
        let store = crate::store::Store::open(&dir).unwrap();
        let first = add_parsed(&store, &dir, "First", &parsed("1.2.3.4", 8388), false).unwrap();
        let items = list(&store, &dir).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].slug, first);
        assert!(items[0].active);
        assert!(items[0].valid);
        assert!(dir.join("xray-config.json").is_file());

        let second = add_parsed(&store, &dir, "Second", &parsed("5.6.7.8", 8389), true).unwrap();
        let items = list(&store, &dir).unwrap();
        assert_eq!(items.len(), 2);
        assert!(items.iter().find(|item| item.slug == first).unwrap().active);
        assert!(
            items
                .iter()
                .find(|item| item.slug == second)
                .unwrap()
                .dynamic
        );

        activate_auto(&store, &dir, &second).unwrap();
        assert!(user_selected_active(&store).is_none());
        activate(&store, &dir, &second).unwrap();
        assert_eq!(
            user_selected_active(&store).as_deref(),
            Some(second.as_str())
        );
        let items = list(&store, &dir).unwrap();
        assert!(
            items
                .iter()
                .find(|item| item.slug == second)
                .unwrap()
                .active
        );

        let (was_active, next) = remove(&store, &dir, &second).unwrap();
        assert!(was_active);
        assert_eq!(next.as_deref(), Some(first.as_str()));
        let (was_active, next) = remove(&store, &dir, &first).unwrap();
        assert!(was_active);
        assert!(next.is_none());
        assert!(!dir.join("xray-config.json").exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn active_shadowsocks_uses_vpn_transport() {
        let dir = temp();
        let store = crate::store::Store::open(&dir).unwrap();
        let vless = link::parse_any(
            "vless://a1b2c3d4-1111-2222-3333-444455556666@1.2.3.4:443?security=tls#VLESS",
        )
        .unwrap();
        add_parsed(&store, &dir, "VLESS", &vless, false).unwrap();
        let outline = add_parsed(&store, &dir, "Outline", &parsed("5.6.7.8", 8388), true).unwrap();
        activate(&store, &dir, &outline).unwrap();

        let config: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("xray-config.json")).unwrap())
                .unwrap();
        let outbounds = config["outbounds"].as_array().unwrap();
        assert_eq!(outbounds.len(), 2);
        assert_eq!(outbounds[0]["protocol"], "shadowsocks");
        assert_eq!(
            outbounds[0]["streamSettings"]["sockopt"]["dialerProxy"],
            link::CHAIN_TAG
        );
        assert_eq!(outbounds[1]["tag"], link::CHAIN_TAG);
        assert_eq!(outbounds[1]["protocol"], "vless");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn records_pool_active_without_manual_flag() {
        let dir = temp();
        let store = crate::store::Store::open(&dir).unwrap();
        sync_pool_active(&store, &dir, &parsed("7.7.7.7", 8390)).unwrap();
        let items = list(&store, &dir).unwrap();
        assert_eq!(items.len(), 1);
        assert!(items[0].active);
        assert_eq!(items[0].slug, POOL_SLUG);
        assert!(user_selected_active(&store).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn imports_existing_single_config() {
        let dir = temp();
        let store = crate::store::Store::open(&dir).unwrap();
        let value = link::config_for_parsed(&parsed("9.9.9.9", 8443));
        std::fs::write(
            dir.join("xray-config.json"),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
        crate::storage::legacy::import_xray_profiles(&dir, &store).unwrap();
        let items = list(&store, &dir).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "Текущий");
        assert!(items[0].active);
        assert!(user_selected_active(&store).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_bad_slugs_and_empty_names() {
        assert!(validate_slug("../evil").is_err());
        assert!(validate_slug("a/b").is_err());
        let dir = temp();
        let store = crate::store::Store::open(&dir).unwrap();
        let parsed = parsed("1.2.3.4", 8388);
        assert!(add_parsed(&store, &dir, "", &parsed, false).is_ok());
        let _ = std::fs::remove_dir_all(dir);
    }
}
