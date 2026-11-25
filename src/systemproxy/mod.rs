//! Системный прокси: Windows (реестр + WinINET) и macOS (networksetup).
//! Windows-часть готова; macOS — в M1.
pub mod platform;

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

#[allow(unused_imports)]
pub use platform::{disable_on_exit, get_system_proxy, set_system_proxy};

use crate::state::AppState;
use crate::wlog;

fn local_domains(state: &AppState) -> Vec<String> {
    state
        .local_domains
        .lock()
        .map(|list| list.iter().map(|item| item.domain.clone()).collect())
        .unwrap_or_default()
}

/// Включает/выключает системный прокси и запоминает выбор пользователя.
pub fn apply(state: &AppState, enabled: bool) -> anyhow::Result<platform::ProxyState> {
    let result = platform::set_system_proxy(enabled, &local_domains(state))?;
    if let Ok(mut settings) = state.settings.lock() {
        settings.system_proxy_enabled = enabled;
        if let Err(e) = settings.save(&state.dir) {
            wlog!("системный прокси: не удалось сохранить настройку — {e:#}");
        }
    }
    Ok(result)
}

/// После запуска возвращает системный прокси, если он был включён до выхода.
pub fn spawn_restore(state: Arc<AppState>) {
    tokio::spawn(async move {
        for _ in 0..50 {
            if state.smart_proxy_ready.load(Ordering::SeqCst) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        if !state.smart_proxy_ready.load(Ordering::SeqCst)
            || !state.settings_snapshot().system_proxy_enabled
        {
            return;
        }
        if get_system_proxy()
            .map(|proxy| proxy.enabled)
            .unwrap_or(false)
        {
            return;
        }
        if let Err(e) = apply(&state, true) {
            wlog!("системный прокси: не удалось восстановить после запуска — {e:#}");
        } else {
            wlog!("системный прокси: восстановлен после запуска");
        }
    });
}
