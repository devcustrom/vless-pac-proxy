pub mod autotune;
pub mod bundle;
pub mod catalog;
pub mod preset;
pub mod service;

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use serde::Serialize;
use serde_json::{json, Value};

use crate::state::AppState;

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
