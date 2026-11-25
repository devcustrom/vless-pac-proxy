pub mod autotune;
pub mod bundle;
pub mod catalog;
pub mod preset;
pub mod service;
pub mod translate;

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde::Serialize;
use serde_json::{json, Value};

use crate::state::AppState;
use crate::wlog;

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct StrategyCandidate {
    pub index: usize,
    pub label: String,
    pub strategy: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AutoTuneState {
    pub running: bool,
    pub phase: String,
    pub log: String,
    pub error: Option<String>,
    pub strategy: Option<String>,
    /// Домен, для которого генерируются стратегии (None — общий автоподбор).
    pub domain: Option<String>,
    pub candidates: Vec<StrategyCandidate>,
    /// Пресет, в который сохранена выбранная стратегия.
    pub preset: Option<String>,
    /// Служба была остановлена на время blockcheck и должна быть возвращена.
    pub service_paused: bool,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

pub fn supported() -> bool {
    cfg!(windows)
}

pub const DEFAULT_TARGETS: [&str; 2] = ["www.youtube.com", "discord.com"];
pub const MAX_TARGETS: usize = 12;

/// Синхронизирует цели Zapret с единым списком доменов.
/// Берём YouTube/Discord и первые домены авто-списка, максимум 12:
/// blockcheck проверяет каждую стратегию по каждому домену.
fn target_list(auto: &[String]) -> Vec<String> {
    let mut targets: Vec<String> = DEFAULT_TARGETS
        .iter()
        .map(|item| item.to_string())
        .collect();
    for domain in auto {
        if targets.len() >= MAX_TARGETS {
            break;
        }
        let domain = domain.trim().to_lowercase();
        if !domain.is_empty() && !targets.iter().any(|item| item == &domain) {
            targets.push(domain);
        }
    }
    targets
}

pub fn sync_domains(state: &AppState, auto: &[String]) {
    let targets = target_list(auto);
    let Ok(mut settings) = state.settings.lock() else {
        return;
    };
    if settings.zapret_domains == targets {
        return;
    }
    settings.zapret_domains = targets;
    if let Err(e) = settings.save(&state.dir) {
        wlog!("zapret: не удалось сохранить цели стратегии — {e:#}");
    }
}

/// Стабильный отпечаток списка доменов для проверки актуальности стратегии.
pub fn domains_stamp(domains: &[String]) -> String {
    use sha2::Digest;
    let mut items: Vec<String> = domains
        .iter()
        .map(|item| item.trim().to_lowercase())
        .filter(|item| !item.is_empty())
        .collect();
    items.sort();
    items.dedup();
    let mut hasher = sha2::Sha256::new();
    for item in items {
        hasher.update(item.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}

pub fn selected_preset(state: &AppState) -> String {
    let settings = state.settings_snapshot();
    let items = preset::list_presets(&state.dir);
    if items.iter().any(|item| item.name == settings.zapret_preset) {
        settings.zapret_preset
    } else {
        items
            .first()
            .map(|item| item.name.clone())
            .unwrap_or_else(|| "general.bat".to_string())
    }
}

pub fn status_json(state: &AppState) -> Value {
    let settings = state.settings_snapshot();
    let bundle = preset::bundle_dir(&state.dir);
    let installed = preset::is_bundle_installed(&state.dir);
    let available = preset::list_presets(&state.dir);
    let presets: Vec<Value> = available
        .iter()
        .map(|item| json!({ "name": item.name, "label": item.label }))
        .collect();
    let selected = selected_preset(state);
    let selected_presets: Vec<String> = if settings.zapret_presets.is_empty() {
        vec![selected.clone()]
    } else {
        settings
            .zapret_presets
            .iter()
            .filter(|name| available.iter().any(|item| &item.name == *name))
            .cloned()
            .collect()
    };
    let service = service::query(&state.dir);
    let installed_bundle = bundle::installed_info(&state.dir);
    let auto = state
        .zapret_auto
        .lock()
        .map(|v| v.clone())
        .unwrap_or_default();
    let error = if supported() {
        state
            .zapret_error
            .lock()
            .ok()
            .and_then(|v| v.clone())
            .or(service.error.clone())
    } else {
        None
    };
    json!({
        "supported": supported(),
        "enabled": settings.zapret_enabled && supported(),
        "bundleInstalled": installed,
        "bundlePath": bundle.display().to_string(),
        "bundle": installed_bundle,
        "engine": preset::engine_label(&state.dir),
        "blockcheck": preset::blockcheck_paths(&state.dir).is_some(),
        "presets": presets,
        "preset": selected,
        "selectedPresets": selected_presets,
        "domains": settings.zapret_domains,
        "strategyStale": service.installed
            && !settings.zapret_strategy_stamp.is_empty()
            && settings.zapret_strategy_stamp != domains_stamp(&settings.zapret_domains),
        "catalog": catalog::status_json(state),
        "service": service,
        "autoTune": auto,
        "error": error,
    })
}

pub fn install(state: &Arc<AppState>, preset_name: &str) -> Result<()> {
    set_error(state, None);
    service::install(&state.dir, preset_name)
}

pub fn remove(state: &Arc<AppState>) -> Result<()> {
    set_error(state, None);
    service::remove(&state.dir)
}

pub fn start(state: &Arc<AppState>) -> Result<String> {
    set_error(state, None);
    service::start()
}

pub fn stop(state: &Arc<AppState>) -> Result<String> {
    set_error(state, None);
    service::stop()
}

pub fn open_folder(state: &AppState) -> Result<()> {
    let path = preset::bundle_dir(&state.dir);
    if !path.is_dir() {
        std::fs::create_dir_all(&path)
            .map_err(|e| anyhow::anyhow!("не удалось создать {}: {e}", path.display()))?;
    }
    crate::shell::open_url(&path.display().to_string())
}

fn set_error(state: &AppState, value: Option<String>) {
    if let Ok(mut guard) = state.zapret_error.lock() {
        *guard = value;
    }
}

pub fn clear_error(state: &AppState) {
    set_error(state, None);
}

pub fn service_running_cached(state: &AppState) -> bool {
    if let Ok(cache) = state.zapret_running.lock() {
        if let Some((running, at)) = *cache {
            if at.elapsed() < Duration::from_secs(5) {
                return running;
            }
        }
    }
    let running = supported() && service::query(&state.dir).running;
    if let Ok(mut cache) = state.zapret_running.lock() {
        *cache = Some((running, Instant::now()));
    }
    running
}

pub fn record_error(state: &AppState, error: &anyhow::Error) {
    set_error(state, Some(format!("{error:#}")));
}

#[cfg(test)]
mod tests {
    use super::domains_stamp;

    fn list(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    #[test]
    fn domains_stamp_is_order_and_case_insensitive() {
        let a = domains_stamp(&list(&["youtube.com", "Discord.com"]));
        let b = domains_stamp(&list(&[" discord.com ", "YOUTUBE.COM", ""]));
        assert_eq!(a, b);
        assert_ne!(a, domains_stamp(&list(&["youtube.com"])));
    }

    #[test]
    fn targets_are_capped_and_deduped() {
        let mut auto = vec!["discord.com".to_string()];
        for index in 0..30 {
            auto.push(format!("site{index}.example"));
        }
        let targets = super::target_list(&auto);
        assert_eq!(targets.len(), super::MAX_TARGETS);
        assert_eq!(targets[0], "www.youtube.com");
        assert_eq!(targets[1], "discord.com");
        assert_eq!(targets[2], "site0.example");
    }
}
