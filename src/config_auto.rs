//! Автоподбор самого быстрого VPN-конфига для каждого канала.
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use crate::state::AppState;
use crate::wlog;

fn network_enabled() -> bool {
    std::env::var_os("WPC_TEST_MODE").is_none()
}

pub fn spawn_once(state: Arc<AppState>) {
    if !network_enabled() || !state.settings_snapshot().auto_rotate {
        return;
    }
    if state.config_auto_running.swap(true, Ordering::SeqCst) {
        return;
    }
    tokio::spawn(async move {
        run_inner(&state).await;
        state.config_auto_running.store(false, Ordering::SeqCst);
    });
}

async fn run_inner(state: &Arc<AppState>) {
    let settings = state.settings_snapshot();
    if !settings.auto_rotate {
        return;
    }
    let mut changed = false;
    match crate::engines::wg_configs::select_fastest(state).await {
        Ok(value) => changed |= value,
        Err(e) => wlog!("автовыбор WireGuard-конфига: {e:#}"),
    }
    // Каналы проверяются независимо; каждый модуль сам пропускает цикл,
    // если пользователь уже запустил ручную проверку.
    match crate::vless::auto::select_fastest(state).await {
        Ok(value) => changed |= value,
        Err(e) => wlog!("автовыбор Xray-конфига: {e:#}"),
    }
    if changed {
        wlog!("автовыбор конфигов: активные конфиги изменены, перепроверяю маршруты");
        crate::route_auto::spawn_check_all(state.clone());
    }
}

pub fn spawn_scheduler(state: Arc<AppState>) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(45)).await;
        loop {
            spawn_once(state.clone());
            tokio::time::sleep(Duration::from_secs(15 * 60)).await;
        }
    });
}
