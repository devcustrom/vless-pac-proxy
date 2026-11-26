//! Простой режим: одна кнопка включения, автоматические подписки и запуск
//! каналов. Расширенный интерфейс остаётся доступен переключателем.
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::state::AppState;
use crate::vless::pool;
use crate::wlog;

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimpleState {
    pub busy: bool,
    pub phase: String,
    pub message: String,
    pub error: Option<String>,
}

fn set_state(state: &AppState, update: impl FnOnce(&mut SimpleState)) {
    if let Ok(mut guard) = state.simple.lock() {
        update(&mut guard);
    }
}

pub fn status(state: &AppState) -> SimpleState {
    state
        .simple
        .lock()
        .map(|item| item.clone())
        .unwrap_or_default()
}

fn set_progress(state: &AppState, phase: &str, message: &str) {
    set_state(state, |item| {
        item.busy = true;
        item.phase = phase.to_string();
        item.message = message.to_string();
        item.error = None;
    });
}

/// Для пустого профиля подтягиваем BLACK-подписки igareck. Это публичные
/// списки, которые уже предлагаются в расширенном интерфейсе.
pub fn default_subscription_urls() -> Vec<String> {
    pool::igareck_group_urls(&["black".to_string()])
}

/// Добавляет стартовые подписки, только если пользователь ещё ничего не задал.
pub fn ensure_sources(state: &AppState) -> bool {
    ensure_sources_in(&state.store)
}

fn ensure_sources_in(store: &crate::store::Store) -> bool {
    if !pool::read_subscriptions(store).is_empty() {
        return false;
    }
    let urls = default_subscription_urls();
    if urls.is_empty() {
        return false;
    }
    match pool::write_subscriptions(store, &urls) {
        Ok(()) => {
            wlog!(
                "простой режим: добавлены подписки igareck BLACK ({} файлов)",
                urls.len()
            );
            true
        }
        Err(e) => {
            wlog!("простой режим: не удалось сохранить подписки — {e:#}");
            false
        }
    }
}

fn wg_configured(state: &AppState) -> bool {
    crate::engines::wireproxy::is_configured(&state.dir)
}

fn xray_configured(state: &AppState) -> bool {
    crate::engines::xray::has_config(&state.dir)
}

fn has_configs(state: &AppState) -> bool {
    wg_configured(state) || xray_configured(state)
}

fn pool_has_working(state: &AppState) -> bool {
    state
        .pool
        .lock()
        .map(|pool| pool.entries.iter().any(|entry| entry.ok))
        .unwrap_or(false)
}

fn pool_testing(state: &AppState) -> bool {
    state.pool.lock().map(|pool| pool.testing).unwrap_or(false)
}

async fn wait_pool(state: &AppState, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while pool_testing(state) {
        if Instant::now() >= deadline {
            bail!("проверка подписок не завершилась за отведённое время");
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    Ok(())
}

async fn wait_xray_engine(state: &AppState) -> Result<()> {
    use crate::engines::{find_engine, EngineKind};
    if find_engine(EngineKind::Xray).is_some() {
        return Ok(());
    }
    set_progress(state, "engine", "Скачиваю движок Xray…");
    for _ in 0..90 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        if find_engine(EngineKind::Xray).is_some() {
            return Ok(());
        }
    }
    bail!("движок Xray ещё скачивается — попробуйте через минуту")
}

/// Гарантирует, что есть хотя бы один рабочий Xray-конфиг. Если конфигов нет
/// вообще, добавляет подписки и проверяет пул.
async fn ensure_vpn_config(state: &Arc<AppState>) -> Result<()> {
    if has_configs(state) {
        return Ok(());
    }
    wait_xray_engine(state).await?;
    if pool_testing(state) {
        set_progress(
            state,
            "pool",
            "Проверяю подписки — это может занять несколько минут…",
        );
        wait_pool(state, Duration::from_secs(600)).await?;
    }
    if !pool_has_working(state) {
        ensure_sources(state);
        set_progress(state, "pool", "Ищу рабочие VPN-конфиги в подписках…");
        pool::refresh(state)
            .await
            .context("не удалось проверить подписки")?;
    }
    let index =
        pool::best_entry_index(state).context("в подписках не нашлось рабочего VPN-конфига")?;
    set_progress(state, "apply", "Подключаю лучший конфиг…");
    pool::apply_entry_checked(state, index)
        .await
        .context("лучший конфиг не прошёл проверку трафика")?;
    Ok(())
}

fn save_channels(state: &Arc<AppState>, wg: bool, xray: bool) -> Result<()> {
    state.update_settings(|settings| {
        settings.wireguard_enabled = wg;
        settings.xray_enabled = xray;
        settings.auto_rotate = true;
        settings.system_proxy_enabled = true;
    })?;
    Ok(())
}

async fn enable_inner(state: &Arc<AppState>) -> Result<()> {
    set_progress(state, "configs", "Проверяю доступные VPN-конфиги…");
    ensure_vpn_config(state).await?;

    let wg = wg_configured(state);
    let xray = xray_configured(state);
    if !wg && !xray {
        bail!("нет ни одного VPN-конфига");
    }
    if xray {
        wait_xray_engine(state).await?;
    }

    save_channels(state, wg, xray)?;
    if wg {
        state.wg.start();
    } else {
        state.wg.stop();
    }
    if xray {
        state.xr.start();
    } else {
        state.xr.stop();
    }

    set_progress(state, "proxy", "Включаю системный прокси…");
    let mut ready = false;
    for _ in 0..50 {
        if state.smart_proxy_ready.load(Ordering::SeqCst) {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    if !ready {
        bail!("умный прокси ещё не готов — попробуйте ещё раз");
    }
    crate::systemproxy::apply(state, true).context("не удалось включить системный прокси")?;

    crate::config_auto::spawn_once(state.clone());
    crate::route_auto::spawn_check_all(state.clone());
    set_state(state, |item| {
        item.busy = false;
        item.phase = "ready".to_string();
        item.message = String::new();
        item.error = None;
    });
    wlog!(
        "простой режим: VPN включён (WireGuard: {}, Xray: {})",
        if wg { "да" } else { "нет" },
        if xray { "да" } else { "нет" }
    );
    Ok(())
}

pub fn spawn_enable(state: Arc<AppState>) {
    if state.simple_running.swap(true, Ordering::SeqCst) {
        return;
    }
    set_progress(&state, "configs", "Проверяю доступные VPN-конфиги…");
    tokio::spawn(async move {
        let result = enable_inner(&state).await;
        if let Err(e) = &result {
            wlog!("простой режим: включение не удалось — {e:#}");
            set_state(&state, |item| {
                item.busy = false;
                item.phase = "error".to_string();
                item.message.clear();
                item.error = Some(format!("{e:#}"));
            });
        }
        state.simple_running.store(false, Ordering::SeqCst);
    });
}

pub fn disable(state: &Arc<AppState>) -> Result<()> {
    let proxy = crate::systemproxy::apply(state, false);
    state.update_settings(|settings| {
        settings.wireguard_enabled = false;
        settings.xray_enabled = false;
        settings.system_proxy_enabled = false;
    })?;
    state.wg.stop();
    state.xr.stop();
    set_state(state, |item| {
        item.busy = false;
        item.phase = "off".to_string();
        item.message.clear();
        item.error = None;
    });
    wlog!("простой режим: VPN выключен");
    proxy
        .map(|_| ())
        .context("не удалось выключить системный прокси")
}

/// При старте в простом режиме без конфигов заранее добавляет подписки и
/// запускает проверку пула, чтобы кнопка «Включить» не ждала впустую.
pub fn spawn_bootstrap(state: Arc<AppState>) {
    if !state.settings_snapshot().simple_ui {
        return;
    }
    tokio::spawn(async move {
        // Даём скачаться движкам и завершиться первичной инициализации.
        tokio::time::sleep(Duration::from_secs(20)).await;
        if has_configs(&state) {
            return;
        }
        ensure_sources(&state);
        if pool_has_working(&state) || pool_testing(&state) {
            return;
        }
        // Не перепроверяем 150 конфигов при каждом запуске, если попытка
        // уже была недавно и ничего не нашла.
        let recently_checked = state
            .pool
            .lock()
            .ok()
            .and_then(|pool| pool.last_attempt_at)
            .map(|at| (chrono::Utc::now() - at).num_seconds() < 3600)
            .unwrap_or(false);
        if recently_checked {
            return;
        }
        if let Err(e) = pool::refresh(&state).await {
            wlog!("простой режим: автозагрузка конфигов — {e:#}");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{default_subscription_urls, ensure_sources_in};

    #[test]
    fn default_sources_are_https_black_lists() {
        let urls = default_subscription_urls();
        assert!(!urls.is_empty());
        assert!(urls.iter().all(|url| url.starts_with("https://")));
        assert!(urls.iter().any(|url| url.contains("BLACK_VLESS_RUS.txt")));
    }

    #[test]
    fn default_sources_are_added_only_once() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "wpc-simple-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = crate::store::Store::open(&dir).unwrap();
        assert!(ensure_sources_in(&store));
        let subs = crate::vless::pool::read_subscriptions(&store);
        assert_eq!(subs, default_subscription_urls());
        assert!(!ensure_sources_in(&store));
        let _ = std::fs::remove_dir_all(dir);
    }
}
