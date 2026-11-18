pub mod config;
pub mod link;
pub mod status;
pub mod supervisor;
pub mod update;

use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Instant;

use anyhow::Result;
use serde::Serialize;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub at: Option<String>,
    pub running: bool,
    pub latency_ms: Option<u64>,
    pub error: Option<String>,
    pub direct: std::collections::BTreeMap<String, bool>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateState {
    pub checking: bool,
    pub last_check: Option<String>,
    pub last_error: Option<String>,
    pub available_version: Option<String>,
    pub installed_version: Option<String>,
}

#[derive(Default)]
pub struct TelegramRuntime {
    /// (generation, stop sender): generation защищает от гонки restart/stop.
    pub stop: Mutex<Option<(u64, tokio::sync::watch::Sender<bool>)>>,
    pub pid: Mutex<Option<u32>>,
    pub error: Mutex<Option<String>>,
    pub generation: AtomicU64,
    pub restarts: AtomicU64,
    pub last_started: Mutex<Option<Instant>>,
    pub probe: Mutex<Option<ProbeResult>>,
    pub update: Mutex<UpdateState>,
    pub updating: std::sync::atomic::AtomicBool,
}

/// Общая точка включения/выключения для API и трея.
pub fn set_enabled(state: &Arc<AppState>, enabled: bool) -> Result<()> {
    {
        let mut candidate = state.settings_snapshot();
        if enabled && !config::valid_secret(&candidate.telegram_secret) {
            candidate.telegram_secret = config::generate_secret();
        }
        candidate.telegram_enabled = enabled;
        if enabled {
            config::TelegramConfig::from_settings(&candidate, state.panel_port)?;
        }
        candidate.save(&state.dir)?;
        *state.settings.lock().unwrap() = candidate;
    }
    if enabled {
        supervisor::start(state)?;
    } else {
        supervisor::stop(state);
    }
    Ok(())
}
