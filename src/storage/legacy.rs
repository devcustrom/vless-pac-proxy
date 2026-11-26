//! One-time migration of legacy text storage into SQLite.
//!
//! The files are moved into a `legacy-domains-<timestamp>/` directory after a
//! successful import. They are kept for rollback/diagnostics and are no longer
//! read by the application.
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::net::local_proxy::LocalDomain;
use crate::store::Store;

const MARKER: &str = "legacy_domains_imported_at";
const DOMAIN_FILES: &[&str] = &[
    "extra-domains.txt",
    "direct-domains.txt",
    "vpn-only-domains.txt",
    "local-domains.txt",
    "subscriptions.txt",
];

pub fn import_domains(dir: &Path, store: &Store) -> Result<()> {
    if store.meta_get(MARKER)?.is_some() {
        return Ok(());
    }
    let current = store.domain_sets()?;
    let has_db_data =
        !current.extra.is_empty() || !current.direct.is_empty() || !current.vpn_only.is_empty();
    let mut imported = false;
    if !has_db_data {
        let sets = crate::store::DomainSets {
            extra: crate::lists::read_domain_file(dir, "extra-domains.txt"),
            direct: crate::lists::read_domain_file(dir, "direct-domains.txt"),
            vpn_only: crate::lists::read_domain_file(dir, "vpn-only-domains.txt"),
        };
        if !sets.extra.is_empty() || !sets.direct.is_empty() || !sets.vpn_only.is_empty() {
            store.replace_domain_sets(&sets)?;
            imported = true;
        }
    }

    if store.local_domains()?.is_empty() {
        if let Ok(path) = crate::paths::in_dir(dir, "local-domains.txt") {
            if let Ok(text) = std::fs::read_to_string(path) {
                let list: Vec<LocalDomain> = crate::net::local_proxy::parse_config(&text);
                if !list.is_empty() {
                    store.replace_local_domains(&list)?;
                    imported = true;
                }
            }
        }
    }

    if store.subscription_urls()?.is_empty() {
        if let Ok(path) = crate::paths::in_dir(dir, "subscriptions.txt") {
            if let Ok(text) = std::fs::read_to_string(path) {
                let urls: Vec<String> = text
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty() && !line.starts_with('#'))
                    .map(str::to_string)
                    .collect();
                if !urls.is_empty() {
                    store.replace_subscription_urls(&urls)?;
                    imported = true;
                }
            }
        }
    }

    if imported {
        archive_files(dir)?;
    }
    store.meta_set(MARKER, &chrono::Utc::now().to_rfc3339())?;
    Ok(())
}

pub fn archive_settings(dir: &Path) {
    let Ok(source) = crate::paths::in_dir(dir, "settings.json") else {
        return;
    };
    if !source.is_file() {
        return;
    }
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let target_dir = dir.join(format!("legacy-settings-{stamp}"));
    if std::fs::create_dir_all(&target_dir).is_err() {
        return;
    }
    let target = target_dir.join("settings.json");
    if let Err(error) = std::fs::rename(&source, &target) {
        if std::fs::copy(&source, &target).is_ok() {
            crate::wlog!(
                "legacy: settings.json скопирован в {}: {error}",
                target.display()
            );
        }
    }
}

fn archive_files(dir: &Path) -> Result<()> {
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let target = dir.join(format!("legacy-domains-{stamp}"));
    let mut moved = 0usize;
    for name in DOMAIN_FILES {
        let Ok(source) = crate::paths::in_dir(dir, name) else {
            continue;
        };
        if !source.is_file() {
            continue;
        }
        if moved == 0 {
            std::fs::create_dir_all(&target)
                .with_context(|| format!("создание {}", target.display()))?;
        }
        let destination: PathBuf = target.join(name);
        if let Err(error) = std::fs::rename(&source, &destination) {
            // A running old process may still hold the file. Keep a copy in
            // the archive and leave the original in place; it is no longer
            // read by the new code.
            std::fs::copy(&source, &destination)
                .with_context(|| format!("копирование {name} в legacy-архив"))?;
            crate::wlog!("legacy: не удалось переместить {name}: {error}");
        }
        moved += 1;
    }
    if moved > 0 {
        crate::wlog!(
            "legacy: импортировано {moved} файлов, архив {}",
            target.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_wg_profiles_and_pool_state() {
        let dir = std::env::temp_dir().join(format!(
            "wpc-legacy-profiles-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("wg-configs")).unwrap();
        std::fs::write(
            dir.join("wg-configs/default.conf"),
            "[Interface]
PrivateKey = abc
Address = 10.0.0.2/32
[Peer]
PublicKey = def
Endpoint = 1.2.3.4:51820
",
        )
        .unwrap();
        std::fs::write(
            dir.join("wg-configs.json"),
            r#"{"active":"default","names":{"default":"Default"}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("pool-state.json"),
            r#"{"updatedAt":null,"activeLink":"vless://test","pool":[]}"#,
        )
        .unwrap();
        let store = Store::open(&dir).unwrap();
        import_wg_profiles(&dir, &store).unwrap();
        import_pool_state(&dir, &store).unwrap();
        let profiles = store.wg_profiles().unwrap();
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].0, "default");
        assert!(profiles[0].3);
        assert!(store
            .pool_state_json()
            .unwrap()
            .unwrap()
            .contains("vless://test"));
        // Idempotent second run.
        import_wg_profiles(&dir, &store).unwrap();
        import_pool_state(&dir, &store).unwrap();
        assert_eq!(store.wg_profiles().unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn imports_once_and_archives_files() {
        let dir = std::env::temp_dir().join(format!(
            "wpc-legacy-domains-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        crate::lists::write_domain_file(
            &dir,
            "extra-domains.txt",
            &["example.com".to_string(), "b.test".to_string()],
        )
        .unwrap();
        crate::lists::write_domain_file(&dir, "direct-domains.txt", &["lan.test".to_string()])
            .unwrap();
        std::fs::write(
            dir.join("local-domains.txt"),
            "panel.localhost = 8080\napp.test = 3000\n",
        )
        .unwrap();
        let store = Store::open(&dir).unwrap();
        import_domains(&dir, &store).unwrap();
        let sets = store.domain_sets().unwrap();
        assert_eq!(sets.extra, ["example.com", "b.test"]);
        assert_eq!(sets.direct, ["lan.test"]);
        assert_eq!(store.local_domains().unwrap().len(), 2);
        assert!(store.meta_get(MARKER).unwrap().is_some());
        assert!(!dir.join("extra-domains.txt").exists());
        assert!(std::fs::read_dir(&dir).unwrap().flatten().any(|entry| entry
            .file_name()
            .to_string_lossy()
            .starts_with("legacy-domains-")));

        // A second run must not duplicate or lose rows.
        import_domains(&dir, &store).unwrap();
        assert_eq!(store.domain_sets().unwrap(), sets);
        assert_eq!(store.local_domains().unwrap().len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[derive(serde::Deserialize, Default)]
struct WgMeta {
    #[serde(default)]
    active: Option<String>,
    #[serde(default)]
    names: std::collections::HashMap<String, String>,
}

/// Imports legacy `wg-configs/` + `wg-configs.json` (+ `wg.conf` fallback)
/// into `wg_profiles` and archives the source files.
pub fn import_wg_profiles(dir: &Path, store: &Store) -> Result<()> {
    const MARKER: &str = "legacy_wg_profiles_imported_at";
    if store.meta_get(MARKER)?.is_some() {
        return Ok(());
    }
    if !store.wg_profiles()?.is_empty() {
        store.meta_set(MARKER, &chrono::Utc::now().to_rfc3339())?;
        return Ok(());
    }
    let meta: WgMeta = std::fs::read_to_string(dir.join("wg-configs.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let configs_dir = dir.join("wg-configs");
    let mut imported = 0usize;
    let mut active_slug = meta.active.clone();
    if let Ok(entries) = std::fs::read_dir(&configs_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|ext| ext != "conf").unwrap_or(true) {
                continue;
            }
            let Some(slug) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let Ok(config) = std::fs::read_to_string(&path) else {
                continue;
            };
            if config.trim().is_empty() {
                continue;
            }
            let name = meta
                .names
                .get(slug)
                .cloned()
                .unwrap_or_else(|| slug.to_string());
            if store.insert_wg_profile(slug, &name, &config).is_ok() {
                imported += 1;
            }
        }
    }
    if imported == 0 {
        if let Ok(config) = std::fs::read_to_string(dir.join("wg.conf")) {
            if !config.trim().is_empty()
                && store
                    .insert_wg_profile("default", "Основной", &config)
                    .is_ok()
            {
                imported = 1;
                active_slug = Some("default".to_string());
            }
        }
    }
    if imported > 0 {
        if let Some(active) = active_slug.as_deref() {
            let _ = store.activate_wg_profile(active);
        }
    }
    if configs_dir.is_dir() {
        archive_path(dir, "wg-configs");
    }
    archive_path(dir, "wg-configs.json");
    store.meta_set(MARKER, &chrono::Utc::now().to_rfc3339())?;
    if imported > 0 {
        crate::wlog!("legacy: импортировано WireGuard-профилей — {imported}");
    }
    Ok(())
}

fn archive_path(dir: &Path, name: &str) -> Option<PathBuf> {
    let source = dir.join(name);
    if !source.exists() {
        return None;
    }
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
    let target_dir = dir.join(format!("legacy-profiles-{stamp}"));
    if std::fs::create_dir_all(&target_dir).is_err() {
        return None;
    }
    let destination = target_dir.join(name);
    if let Err(error) = std::fs::rename(&source, &destination) {
        if std::fs::copy(&source, &destination).is_err() {
            crate::wlog!("legacy: не удалось архивировать {name}: {error}");
            return None;
        }
    }
    Some(destination)
}

#[derive(serde::Deserialize, Default)]
struct XrayMeta {
    #[serde(default)]
    active: Option<String>,
    #[serde(default)]
    user_selected: bool,
    #[serde(default)]
    names: std::collections::HashMap<String, String>,
    #[serde(default)]
    items: std::collections::HashMap<String, XrayMetaItem>,
}

#[derive(serde::Deserialize, Default)]
struct XrayMetaItem {
    #[serde(default)]
    protocol: String,
    #[serde(default)]
    address: String,
    #[serde(default)]
    port: u16,
    #[serde(default)]
    dynamic: bool,
}

fn xray_info(config: &serde_json::Value) -> Option<(String, String, u16)> {
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
    Some((protocol, address, port as u16))
}

/// Imports legacy `xray-configs/*.json`, metadata and the single active
/// `xray-config.json` into `xray_profiles`.
pub fn import_xray_profiles(dir: &Path, store: &Store) -> Result<()> {
    const MARKER: &str = "legacy_xray_profiles_imported_at";
    if store.meta_get(MARKER)?.is_some() {
        return Ok(());
    }
    if !store.xray_profiles()?.is_empty() {
        store.meta_set(MARKER, &chrono::Utc::now().to_rfc3339())?;
        return Ok(());
    }
    let meta: XrayMeta = std::fs::read_to_string(dir.join("xray-configs.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let configs_dir = dir.join("xray-configs");
    let mut slugs: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&configs_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|ext| ext != "json").unwrap_or(true) {
                continue;
            }
            if let Some(slug) = path.file_stem().and_then(|stem| stem.to_str()) {
                slugs.push(slug.to_string());
            }
        }
    }
    let mut imported = 0usize;
    for slug in &slugs {
        let path = configs_dir.join(format!("{slug}.json"));
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(config) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let stored = meta.items.get(slug);
        let info = xray_info(&config);
        let protocol = stored
            .map(|item| item.protocol.clone())
            .filter(|value| !value.is_empty())
            .or_else(|| info.as_ref().map(|item| item.0.clone()))
            .unwrap_or_default();
        let address = stored
            .map(|item| item.address.clone())
            .filter(|value| !value.is_empty())
            .or_else(|| info.as_ref().map(|item| item.1.clone()))
            .unwrap_or_default();
        let port = stored
            .map(|item| item.port)
            .filter(|value| *value != 0)
            .or_else(|| info.as_ref().map(|item| item.2))
            .unwrap_or(0);
        let name = meta
            .names
            .get(slug)
            .cloned()
            .unwrap_or_else(|| slug.clone());
        let dynamic = stored.map(|item| item.dynamic).unwrap_or(false) || slug == "pool-active";
        if store
            .insert_xray_profile(slug, &name, &text, &protocol, &address, port, dynamic)
            .is_ok()
        {
            imported += 1;
        }
    }
    if imported == 0 {
        if let Ok(text) = std::fs::read_to_string(dir.join("xray-config.json")) {
            if let Ok(config) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some((protocol, address, port)) = xray_info(&config) {
                    if store
                        .insert_xray_profile(
                            "current",
                            "Текущий",
                            &text,
                            &protocol,
                            &address,
                            port,
                            false,
                        )
                        .is_ok()
                    {
                        imported = 1;
                    }
                }
            }
        }
    }
    if imported > 0 {
        let active = meta
            .active
            .filter(|slug| slugs.iter().any(|item| item == slug))
            .or_else(|| slugs.first().cloned())
            .unwrap_or_else(|| "current".to_string());
        let _ = store.activate_xray_profile(&active, meta.user_selected);
    }
    if configs_dir.is_dir() {
        archive_path(dir, "xray-configs");
    }
    archive_path(dir, "xray-configs.json");
    store.meta_set(MARKER, &chrono::Utc::now().to_rfc3339())?;
    if imported > 0 {
        crate::wlog!("legacy: импортировано Xray-профилей — {imported}");
    }
    Ok(())
}

/// Imports the legacy pool cache and active link into SQLite.
pub fn import_pool_state(dir: &Path, store: &Store) -> Result<()> {
    const MARKER: &str = "legacy_pool_state_imported_at";
    if store.meta_get(MARKER)?.is_some() {
        return Ok(());
    }
    if store.pool_state_json()?.is_none() {
        if let Ok(text) = std::fs::read_to_string(dir.join("pool-state.json")) {
            if serde_json::from_str::<serde_json::Value>(&text).is_ok() {
                store.save_pool_state_json(&text)?;
            }
        }
    }
    archive_path(dir, "pool-state.json");
    store.meta_set(MARKER, &chrono::Utc::now().to_rfc3339())?;
    Ok(())
}
