//! HTTP-панель и API на 127.0.0.1:8080 + запуск всех сервисов менеджера.
use std::sync::Arc;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Request, State};
use axum::http::{header, Method, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use include_dir::{include_dir, Dir};
use serde_json::{json, Value};

use crate::engines::EngineKind;
use crate::state::{AppState, CheckEntry};
use crate::wlog;

pub const SMART_PROXY_PORT: u16 = 8081;
pub const PANEL_PORT: u16 = 8080;

/// Порт умного прокси. В E2E задаётся отдельный, чтобы не конфликтовать
/// с уже запущенным рабочим экземпляром.
pub fn smart_proxy_port() -> u16 {
    std::env::var("WPC_SMART_PROXY_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|port| *port > 0)
        .unwrap_or(SMART_PROXY_PORT)
}

#[cfg(unix)]
async fn shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    match signal(SignalKind::terminate()) {
        Ok(mut term) => {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
        }
        Err(e) => {
            wlog!("SIGTERM handler: {e}");
            let _ = tokio::signal::ctrl_c().await;
        }
    }
}

#[cfg(not(unix))]
async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

/// Собранный фронтенд (Vue + Tailwind + shadcn): `npm --prefix web run build`.
static UI: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/web/dist");

pub async fn run(state: Arc<AppState>) -> anyhow::Result<()> {
    let panel_port = state.panel_port;
    let test_mode = std::env::var_os("WPC_TEST_MODE").is_some();
    wlog!(
        "панель: http://127.0.0.1:{panel_port}/  |  умный прокси: 127.0.0.1:{}",
        smart_proxy_port()
    );

    crate::route_auto::migrate_lists(&state);
    crate::route_auto::reconcile_lists(&state);
    crate::route_auto::prune_decisions(&state);
    crate::zapret::sync_domains(&state, &crate::route_auto::auto_domains(&state));

    // движки: скачиваем отсутствующие, затем запускаем супервизор
    if !test_mode {
        let st = state.clone();
        tokio::spawn(async move {
            let missing = [EngineKind::Wireproxy, EngineKind::Xray]
                .into_iter()
                .any(|kind| crate::engines::find_engine(kind).is_none());
            if missing {
                wlog!("движки: скачиваю отсутствующие (первый запуск)…");
                for kind in [EngineKind::Wireproxy, EngineKind::Xray] {
                    if let Err(e) =
                        crate::engines::downloads::ensure(&st.dir, kind, Some(st.clone())).await
                    {
                        wlog!("движки: {} — {e:#}", kind.label());
                    }
                }
            }
            start_enabled_engines(&st);
        });
    } else {
        start_enabled_engines(&state);
    }
    {
        let st = state.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::net::smart_proxy::serve(st).await {
                wlog!("умный прокси остановлен: {e}");
            }
        });
    }
    if !test_mode {
        crate::systemproxy::spawn_restore(state.clone());
    }
    if state.settings_snapshot().local_domains_enabled {
        crate::net::local_proxy::preflight_ports(&state).await;
        crate::net::local_proxy::start(&state);
    }
    crate::zapret::bundle::recover(&state.dir);
    if state.settings_snapshot().telegram_enabled {
        if let Err(e) = crate::telegram::supervisor::start(&state) {
            wlog!("telegram: автозапуск не удался — {e:#}");
        }
    }
    if !test_mode {
        crate::telegram::update::spawn_scheduler(state.clone());
        crate::lists::spawn_scheduler(state.clone());
        crate::vless::pool::spawn_scheduler(state.clone());
        crate::site_check::spawn_scheduler(state.clone());
        crate::route_auto::spawn_scheduler(state.clone());
        crate::config_auto::spawn_scheduler(state.clone());
        crate::simple::spawn_bootstrap(state.clone());
        spawn_health_probes(state.clone());
    }

    {
        let st = state.clone();
        tokio::spawn(async move {
            shutdown_signal().await;
            wlog!("завершение по сигналу");
            cleanup(&st);
            tokio::time::sleep(Duration::from_millis(700)).await;
            std::process::exit(0);
        });
    }

    let app = Router::new()
        .route("/", get(index))
        .route("/api/status", get(status))
        .route("/proxy.pac", get(pac))
        .route("/api/extra", post(extra))
        .route("/api/vless", post(vless))
        .route(
            "/api/xray/configs",
            get(xray_configs_get).post(xray_configs_post),
        )
        .route("/api/wg", post(wg_config))
        .route("/api/wg/configs", get(wg_configs_get).post(wg_configs_post))
        .route("/api/refresh", post(refresh_lists))
        .route("/api/system-proxy", post(system_proxy))
        .route("/api/quit", post(quit))
        .route("/api/auto", post(auto))
        .route("/api/channels", post(channels))
        .route("/api/onboarding", post(onboarding_done))
        .route("/api/ui-mode", post(ui_mode))
        .route("/api/simple/enable", post(simple_enable))
        .route("/api/simple/disable", post(simple_disable))
        .route(
            "/api/local-domains",
            get(local_domains_get).post(local_domains_post),
        )
        .route("/api/local-domains/probe", post(local_domains_probe))
        .route("/api/local-domains/hosts", post(local_domains_hosts))
        .route(
            "/api/local-domains/hosts/apply",
            post(local_domains_hosts_apply),
        )
        .route(
            "/api/local-domains/hosts/edit",
            post(local_domains_hosts_edit),
        )
        .route("/api/local-ports", get(local_ports))
        .route("/api/check", post(check))
        .route("/api/routes/check", post(routes_check))
        .route("/api/configs/auto", post(configs_auto))
        .route("/api/pool/refresh", post(pool_refresh))
        .route("/api/pool/auto", post(pool_auto))
        .route("/api/pool/use", post(pool_use))
        .route("/api/subscriptions", post(subscriptions))
        .route("/api/engines/ensure", post(engines_ensure))
        .route("/api/trust-certs", post(trust_certs_handler))
        .route("/api/plugins", get(plugins_get).post(plugins_set))
        .route("/api/autostart", post(autostart_set))
        .route("/api/hwid", post(hwid_set))
        .route("/api/telegram", get(telegram_get).post(telegram_post))
        .route("/api/telegram/link", post(telegram_link))
        .route("/api/zapret", get(zapret_get).post(zapret_post))
        .route("/api/docs", get(docs))
        .route("/api/update", get(update_check))
        .route("/api/update/install", post(update_install))
        .route("/api/backup", get(backup_download))
        .route("/api/backup/restore", post(backup_restore))
        .fallback(ui_fallback)
        .with_state(state.clone())
        .layer(DefaultBodyLimit::max(100 * 1024 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), guard));

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", panel_port)).await?;
    crate::paths::write_panel_port(&state.dir, panel_port);
    if state.interactive {
        // Панель уже слушает — можно сразу открыть браузер, не дожидаясь
        // скачивания движков на первом запуске.
        let url = format!("http://127.0.0.1:{panel_port}/");
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(300)).await;
            if let Err(e) = crate::shell::open_url(&url) {
                wlog!("панель: не удалось открыть браузер — {e:#}");
            }
        });
    }
    axum::serve(listener, app).await?;
    Ok(())
}

/// Завершение: выключить системный прокси и остановить движки.
pub fn cleanup(state: &AppState) {
    crate::systemproxy::disable_on_exit();
    state.wg.shutdown();
    state.xr.shutdown();
    crate::telegram::supervisor::stop(state);
    crate::vless::pool::cleanup_test_processes();
    crate::paths::clear_panel_port(&state.dir);
    wlog!("выход: системный прокси выключен, движки останавливаются");
}

/// Перед заменой бинарника: остановить движки, но оставить системный прокси
/// включённым — новая версия поднимет 8081 через секунду.
pub fn cleanup_for_update(state: &AppState) {
    state.wg.shutdown();
    state.xr.shutdown();
    crate::telegram::supervisor::stop(state);
    crate::net::local_proxy::stop(state);
    crate::vless::pool::cleanup_test_processes();
    wlog!("обновление: движки остановлены, системный прокси оставлен");
}

async fn quit(State(st): State<Arc<AppState>>) -> Json<Value> {
    let st = st.clone();
    tokio::spawn(async move {
        wlog!("API: получен запрос на завершение");
        cleanup(&st);
        tokio::time::sleep(Duration::from_millis(600)).await;
        std::process::exit(0);
    });
    Json(json!({ "ok": true }))
}

async fn plugins_get(State(st): State<Arc<AppState>>) -> Json<Value> {
    let settings = st.settings_snapshot();
    Json(json!({
        "localDomains": settings.local_domains_enabled,
        "telegram": settings.telegram_enabled,
        "zapret": settings.zapret_enabled && crate::zapret::supported(),
    }))
}

async fn autostart_set(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let Some(enabled) = body.get("enabled").and_then(Value::as_bool) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "нужно enabled: true|false" })),
        )
            .into_response();
    };
    let result = if enabled {
        crate::autostart::install(&st.dir)
    } else {
        crate::autostart::uninstall()
    };
    match result {
        Ok(()) => {
            #[cfg(windows)]
            let (exe, restart_required) = {
                let stable = crate::autostart::stable_exe_path(&st.dir);
                let current = std::env::current_exe().ok();
                (
                    stable.display().to_string(),
                    stable.is_file() && current.as_deref() != Some(stable.as_path()),
                )
            };
            #[cfg(not(windows))]
            let (exe, restart_required) = (String::new(), false);
            Json(json!({
                "ok": true,
                "autostart": enabled,
                "exe": exe,
                "restartRequired": restart_required,
            }))
            .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("{e:#}") })),
        )
            .into_response(),
    }
}

async fn hwid_set(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let requested = body.get("hwid").and_then(Value::as_str);
    let value = match requested {
        Some(raw) if !raw.trim().is_empty() => {
            let value = raw.trim().to_string();
            if !crate::settings::valid_hwid(&value) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "HWID: 8..128 символов, только буквы/цифры/-/_" })),
                )
                    .into_response();
            }
            value
        }
        _ => crate::settings::generate_hwid(),
    };
    let mut candidate = st.settings_snapshot();
    candidate.hwid = value.clone();
    if let Err(e) = candidate.save(&st.dir) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response();
    }
    *st.settings.lock().unwrap() = candidate;
    wlog!("HWID обновлён");
    Json(json!({ "ok": true, "hwid": value })).into_response()
}

async fn plugins_set(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    // Zapret можно переключать отдельно, без localDomains/telegram.
    if body.get("localDomains").is_none() && body.get("telegram").is_none() {
        if let Some(enabled) = body.get("zapret").and_then(|value| value.as_bool()) {
            if enabled && !crate::zapret::supported() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "Zapret поддерживается только на Windows" })),
                )
                    .into_response();
            }
            let result = {
                let mut settings = st.settings.lock().unwrap();
                settings.zapret_enabled = enabled;
                settings.save(&st.dir).map_err(|e| e.to_string())
            };
            if let Err(e) = result {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e })),
                )
                    .into_response();
            }
            return Json(crate::zapret::status_json(&st)).into_response();
        }
    }
    // Telegram-плагин можно переключать отдельно, не передавая localDomains.
    if body.get("localDomains").is_none() {
        if let Some(enabled) = body.get("telegram").and_then(|value| value.as_bool()) {
            let save_result = {
                let mut settings = st.settings.lock().unwrap();
                settings.telegram_enabled = enabled;
                if enabled && !crate::telegram::config::valid_secret(&settings.telegram_secret) {
                    settings.telegram_secret = crate::telegram::config::generate_secret();
                }
                settings.save(&st.dir).map_err(|e| e.to_string())
            };
            if let Err(e) = save_result {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e })),
                )
                    .into_response();
            }
            if enabled {
                if let Err(e) = crate::telegram::supervisor::start(&st) {
                    wlog!("telegram: запуск не удался — {e:#}");
                }
                tokio::time::sleep(Duration::from_millis(400)).await;
            } else {
                crate::telegram::supervisor::stop(&st);
            }
            return Json(crate::telegram::status::status_json(&st)).into_response();
        }
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "нужно localDomains или telegram: true|false" })),
        )
            .into_response();
    }
    let Some(enabled) = body.get("localDomains").and_then(|value| value.as_bool()) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "нужно localDomains: true|false" })),
        )
            .into_response();
    };
    let panel_domain = match body.get("panelDomain").and_then(|value| value.as_str()) {
        Some(raw) => match crate::lists::validate_hostname(raw) {
            Some(domain) => Some(domain),
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "некорректный домен панели" })),
                )
                    .into_response()
            }
        },
        None => None,
    };
    {
        let mut candidate = st.settings_snapshot();
        candidate.local_domains_enabled = enabled;
        if let Err(e) = candidate.save(&st.dir) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response();
        }
        *st.settings.lock().unwrap() = candidate;
    }
    let mut chosen_domain = None;
    if enabled {
        let chosen = panel_domain.unwrap_or_else(|| "panel.localhost".to_string());
        let mut list = read_local_domains(&st);
        list.retain(|item| item.port != st.panel_port && item.domain != chosen);
        list.push(crate::net::local_proxy::LocalDomain {
            domain: chosen.clone(),
            port: st.panel_port,
        });
        if let Err(e) = write_local_domains(&st.dir, &list) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response();
        }
        if let Ok(mut current) = st.local_domains.lock() {
            *current = list;
        }
        // Если слушатели уже работают, preflight увидит их же порты занятыми
        // и сохранит другие значения, не перезапустив listener.
        if !crate::net::local_proxy::is_running(&st) {
            crate::net::local_proxy::preflight_ports(&st).await;
        }
        reload_local_tls(&st).await;
        crate::net::local_proxy::start(&st);
        // даём слушателям шанс занять порты, чтобы вернуть ошибки
        tokio::time::sleep(Duration::from_millis(300)).await;
        chosen_domain = Some(chosen);
    } else {
        crate::net::local_proxy::stop(&st);
    }
    let domains: Vec<String> = st
        .local_domains
        .lock()
        .map(|list| list.iter().map(|item| item.domain.clone()).collect())
        .unwrap_or_default();
    if crate::systemproxy::get_system_proxy()
        .map(|proxy| proxy.enabled)
        .unwrap_or(false)
    {
        let bypass: &[String] = if enabled { &domains } else { &[] };
        if let Err(e) = crate::systemproxy::set_system_proxy(true, bypass) {
            wlog!("системный прокси: исключения плагина — {e:#}");
        }
    }
    let http_error = st
        .local_http_error
        .lock()
        .ok()
        .and_then(|value| value.clone());
    let https_error = st
        .local_https_error
        .lock()
        .ok()
        .and_then(|value| value.clone());
    wlog!(
        "плагин локальных доменов: {}",
        if enabled {
            "включён"
        } else {
            "выключен"
        }
    );
    Json(json!({
        "ok": true,
        "localDomains": enabled,
        "panelDomain": chosen_domain,
        "caTrusted": crate::trust_status::is_trusted(&st.dir),
        "httpError": http_error,
        "httpsError": https_error,
    }))
    .into_response()
}

async fn telegram_get(State(st): State<Arc<AppState>>) -> Json<Value> {
    Json(crate::telegram::status::status_json(&st))
}

fn apply_telegram_set(
    settings: &mut crate::settings::Settings,
    body: &Value,
) -> Result<(), String> {
    if let Some(value) = body.get("port") {
        let port = value
            .as_u64()
            .and_then(|p| u16::try_from(p).ok())
            .filter(|p| *p > 0)
            .ok_or_else(|| "некорректный порт".to_string())?;
        settings.telegram_port = port;
    }
    if let Some(value) = body.get("tryDirect").and_then(Value::as_bool) {
        settings.telegram_try_direct = value;
    }
    if let Some(value) = body.get("order").and_then(Value::as_str) {
        settings.telegram_order = value.trim().to_string();
    }
    if let Some(value) = body.get("cfWorker").and_then(Value::as_str) {
        crate::telegram::config::validate_domains(value).map_err(|e| format!("{e:#}"))?;
        settings.telegram_cf_worker = value.trim().to_string();
    }
    if let Some(value) = body.get("cfProxy").and_then(Value::as_str) {
        crate::telegram::config::validate_domains(value).map_err(|e| format!("{e:#}"))?;
        settings.telegram_cf_proxy = value.trim().to_string();
    }
    if let Some(value) = body.get("fakeTls").and_then(Value::as_str) {
        crate::telegram::config::validate_domains(value).map_err(|e| format!("{e:#}"))?;
        settings.telegram_fake_tls = value.trim().to_string();
    }
    if let Some(value) = body.get("h2").and_then(Value::as_bool) {
        settings.telegram_h2 = value;
    }
    if let Some(value) = body.get("verbose").and_then(Value::as_bool) {
        settings.telegram_verbose = value;
    }
    if let Some(value) = body.get("autoUpdate").and_then(Value::as_bool) {
        settings.telegram_auto_update = value;
    }
    if settings.telegram_secret.is_empty() {
        settings.telegram_secret = crate::telegram::config::generate_secret();
    }
    crate::telegram::config::order_for(settings).map_err(|e| format!("{e:#}"))?;
    Ok(())
}

async fn telegram_post(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let action = body.get("action").and_then(Value::as_str).unwrap_or("");
    match action {
        "enable" | "disable" => {
            let enabled = action == "enable";
            let result: Result<(), String> = {
                let mut settings = st.settings.lock().unwrap();
                if enabled && !crate::telegram::config::valid_secret(&settings.telegram_secret) {
                    settings.telegram_secret = crate::telegram::config::generate_secret();
                }
                settings.telegram_enabled = enabled;
                if enabled {
                    match crate::telegram::config::TelegramConfig::from_settings(
                        &settings,
                        st.panel_port,
                    ) {
                        Ok(_) => settings.save(&st.dir).map_err(|e| e.to_string()),
                        Err(e) => Err(format!("{e:#}")),
                    }
                } else {
                    settings.save(&st.dir).map_err(|e| e.to_string())
                }
            };
            if let Err(e) = result {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": e })),
                )
                    .into_response();
            }
            if enabled {
                if let Err(e) = crate::telegram::supervisor::start(&st) {
                    wlog!("telegram: запуск не удался — {e:#}");
                }
                tokio::time::sleep(Duration::from_millis(400)).await;
            } else {
                crate::telegram::supervisor::stop(&st);
            }
            Json(crate::telegram::status::status_json(&st)).into_response()
        }
        "restart" => {
            if let Err(e) = crate::telegram::supervisor::restart(&st).await {
                wlog!("telegram: перезапуск не удался — {e:#}");
            }
            Json(crate::telegram::status::status_json(&st)).into_response()
        }
        "probe" => {
            crate::telegram::supervisor::probe(&st).await;
            Json(crate::telegram::status::status_json(&st)).into_response()
        }
        "probe-direct" => {
            crate::telegram::supervisor::probe_direct(&st).await;
            Json(crate::telegram::status::status_json(&st)).into_response()
        }
        "rotate-secret" => {
            let result: Result<(), String> = {
                let mut settings = st.settings.lock().unwrap();
                settings.telegram_secret = crate::telegram::config::generate_secret();
                settings.save(&st.dir).map_err(|e| e.to_string())
            };
            if let Err(e) = result {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e })),
                )
                    .into_response();
            }
            if st.settings_snapshot().telegram_enabled {
                if let Err(e) = crate::telegram::supervisor::restart(&st).await {
                    wlog!("telegram: смена секрета — {e:#}");
                }
            }
            Json(crate::telegram::status::status_json(&st)).into_response()
        }
        "ensure-sidecar" => {
            let path = crate::telegram::supervisor::sidecar_path(&st.dir);
            if path.is_file() {
                Json(json!({ "ok": true, "installed": true })).into_response()
            } else {
                match crate::telegram::update::check_and_update(&st, true).await {
                    Ok(version) => Json(json!({
                        "ok": true,
                        "installed": true,
                        "version": version,
                    }))
                    .into_response(),
                    Err(e) => (
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "error": format!("{e:#}"),
                            "path": path.display().to_string(),
                        })),
                    )
                        .into_response(),
                }
            }
        }
        "check-update" => match crate::telegram::update::check_and_update(&st, true).await {
            Ok(version) => Json(json!({
                "ok": true,
                "version": version,
                "telegram": crate::telegram::status::status_json(&st),
            }))
            .into_response(),
            Err(e) => {
                let text = format!("{e:#}");
                if text.contains("HTTP 404") {
                    crate::telegram::update::clear_update_error(&st);
                    return Json(json!({
                        "ok": true,
                        "published": false,
                        "version": crate::telegram::update::installed_version(&st.dir),
                        "message": "манифест обновлений ещё не опубликован",
                    }))
                    .into_response();
                }
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": text })),
                )
                    .into_response()
            }
        },
        "set" => {
            let mut settings = st.settings_snapshot();
            if let Err(e) = apply_telegram_set(&mut settings, &body) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": e })),
                )
                    .into_response();
            }
            if let Err(e) =
                crate::telegram::config::TelegramConfig::from_settings(&settings, st.panel_port)
            {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("{e:#}") })),
                )
                    .into_response();
            }
            if let Err(e) = settings.save(&st.dir) {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
                    .into_response();
            }
            {
                let mut current = st.settings.lock().unwrap();
                *current = settings.clone();
            }
            if settings.telegram_enabled {
                if let Err(e) = crate::telegram::supervisor::restart(&st).await {
                    wlog!("telegram: применение настроек — {e:#}");
                }
            }
            Json(crate::telegram::status::status_json(&st)).into_response()
        }
        _ => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "action должен быть enable|disable|restart|set|probe|probe-direct|ensure-sidecar|check-update|rotate-secret"
            })),
        )
            .into_response(),
    }
}

async fn telegram_link(State(st): State<Arc<AppState>>) -> Response {
    let settings = st.settings_snapshot();
    if !settings.telegram_enabled {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Telegram-прокси выключен" })),
        )
            .into_response();
    }
    match crate::telegram::link::build_link(
        &settings.telegram_host,
        settings.telegram_port,
        &settings.telegram_secret,
        &settings.telegram_fake_tls,
    ) {
        Ok(link) => Json(json!({ "ok": true, "link": link })).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("{e:#}") })),
        )
            .into_response(),
    }
}

async fn zapret_get(State(st): State<Arc<AppState>>) -> Json<Value> {
    Json(crate::zapret::status_json(&st))
}

async fn zapret_post(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let action = body.get("action").and_then(Value::as_str).unwrap_or("");
    match action {
        "download" => {
            if !crate::zapret::supported() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "Zapret поддерживается только на Windows" })),
                )
                    .into_response();
            }
            let variant = body
                .get("variant")
                .and_then(Value::as_str)
                .and_then(crate::zapret::bundle::BundleVariant::parse)
                .unwrap_or(crate::zapret::bundle::BundleVariant::Zapret2);
            match crate::zapret::bundle::install(&st, variant).await {
                Ok(info) => {
                    crate::zapret::clear_error(&st);
                    tokio::time::sleep(Duration::from_millis(300)).await;
                    Json(json!({
                        "ok": true,
                        "bundle": info,
                        "status": crate::zapret::status_json(&st),
                    }))
                    .into_response()
                }
                Err(e) => {
                    crate::zapret::record_error(&st, &e);
                    (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": format!("{e:#}") })),
                    )
                        .into_response()
                }
            }
        }
        "install" => {
            let available = crate::zapret::preset::list_presets(&st.dir);
            let mut names: Vec<String> = body
                .get("presets")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::trim)
                        .filter(|name| !name.is_empty())
                        .map(ToString::to_string)
                        .collect()
                })
                .unwrap_or_default();
            let mut seen = std::collections::HashSet::new();
            names.retain(|name| seen.insert(name.clone()));
            if names.is_empty() {
                let preset = body
                    .get("preset")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                names.push(if preset.is_empty() {
                    crate::zapret::selected_preset(&st)
                } else {
                    preset.to_string()
                });
            }
            if names.iter().any(|name| {
                !crate::zapret::preset::valid_preset_name(&st.dir, name)
                    || !available.iter().any(|item| &item.name == name)
            }) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "некорректный или неизвестный пресет Zapret" })),
                )
                    .into_response();
            }
            let merged = if names.len() > 1 {
                match crate::zapret::preset::parse_presets(&st.dir, &names) {
                    Ok(args) => Some(args),
                    Err(e) => {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(json!({ "error": format!("{e:#}") })),
                        )
                            .into_response()
                    }
                }
            } else {
                None
            };
            {
                let mut settings = st.settings.lock().unwrap();
                settings.zapret_preset = names[0].clone();
                settings.zapret_presets = names.clone();
                settings.zapret_catalog_strategy.clear();
                if let Err(e) = settings.save(&st.dir) {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "error": e.to_string() })),
                    )
                        .into_response();
                }
            }
            let result = match merged {
                Some(args) => crate::zapret::preset::ensure_user_lists(&st.dir)
                    .and_then(|_| crate::zapret::service::install_args(&st.dir, args)),
                None => crate::zapret::install(&st, &names[0]),
            };
            match result {
                Ok(()) => {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    crate::route_auto::spawn_check_all(st.clone());
                    Json(json!({
                        "ok": true,
                        "message": "установщик службы запущен (подтвердите UAC)",
                        "status": crate::zapret::status_json(&st),
                    }))
                    .into_response()
                }
                Err(e) => {
                    crate::zapret::record_error(&st, &e);
                    (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": format!("{e:#}") })),
                    )
                        .into_response()
                }
            }
        }
        "remove" => match crate::zapret::remove(&st) {
            Ok(()) => {
                tokio::time::sleep(Duration::from_millis(300)).await;
                crate::route_auto::spawn_check_all(st.clone());
                Json(json!({
                    "ok": true,
                    "message": "удаление службы запущено (подтвердите UAC)",
                    "status": crate::zapret::status_json(&st),
                }))
                .into_response()
            }
            Err(e) => {
                crate::zapret::record_error(&st, &e);
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("{e:#}") })),
                )
                    .into_response()
            }
        },
        "start" => match crate::zapret::start(&st) {
            Ok(_) => {
                tokio::time::sleep(Duration::from_millis(400)).await;
                crate::route_auto::spawn_check_all(st.clone());
                Json(crate::zapret::status_json(&st)).into_response()
            }
            Err(e) => {
                crate::zapret::record_error(&st, &e);
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("{e:#}") })),
                )
                    .into_response()
            }
        },
        "stop" => match crate::zapret::stop(&st) {
            Ok(_) => {
                tokio::time::sleep(Duration::from_millis(400)).await;
                crate::route_auto::spawn_check_all(st.clone());
                Json(crate::zapret::status_json(&st)).into_response()
            }
            Err(e) => {
                crate::zapret::record_error(&st, &e);
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("{e:#}") })),
                )
                    .into_response()
            }
        },
        "set-preset" => {
            let Some(preset) = body.get("preset").and_then(Value::as_str) else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "нужен preset" })),
                )
                    .into_response();
            };
            let available = crate::zapret::preset::list_presets(&st.dir);
            if !crate::zapret::preset::valid_preset_name(&st.dir, preset)
                || !available.iter().any(|item| item.name == preset)
            {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "неизвестный пресет Zapret" })),
                )
                    .into_response();
            }
            {
                let mut settings = st.settings.lock().unwrap();
                settings.zapret_preset = preset.to_string();
                if let Err(e) = settings.save(&st.dir) {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({ "error": e.to_string() })),
                    )
                        .into_response();
                }
            }
            Json(crate::zapret::status_json(&st)).into_response()
        }
        "open-folder" => match crate::zapret::open_folder(&st) {
            Ok(()) => Json(json!({ "ok": true })).into_response(),
            Err(e) => (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("{e:#}") })),
            )
                .into_response(),
        },
        "probe" => {
            if !crate::zapret::service_running_cached(&st) {
                return Json(json!({
                    "ok": false,
                    "running": false,
                    "error": "служба Zapret не запущена — установите и запустите её",
                }))
                .into_response();
            }
            let client = st.client("zapret-probe", None);
            let mut domains = st.settings_snapshot().zapret_domains;
            if domains.is_empty() {
                domains = vec!["www.youtube.com".into(), "discord.com".into()];
            }
            let targets: Vec<(String, String)> = domains
                .into_iter()
                .take(8)
                .map(|domain| (domain.clone(), format!("https://{domain}/")))
                .collect();
            let mut results = Vec::new();
            for (name, url) in targets {
                let started = std::time::Instant::now();
                let response = tokio::time::timeout(
                    Duration::from_secs(8),
                    client.get(url).send(),
                )
                .await;
                let (code, error) = match response {
                    Ok(Ok(resp)) => (resp.status().as_u16().to_string(), None),
                    Ok(Err(e)) => ("000".to_string(), Some(e.to_string())),
                    Err(_) => ("000".to_string(), Some("таймаут".to_string())),
                };
                results.push(json!({
                    "name": name,
                    "code": code,
                    "ms": started.elapsed().as_millis() as u64,
                    "error": error,
                }));
            }
            Json(json!({
                "ok": true,
                "running": true,
                "targets": results,
            }))
            .into_response()
        }
        "auto-tune" => match crate::zapret::autotune::start(&st) {
            Ok(()) => Json(json!({
                "ok": true,
                "message": "автоподбор запущен (подтвердите UAC)",
                "status": crate::zapret::status_json(&st),
            }))
            .into_response(),
            Err(e) => {
                crate::zapret::record_error(&st, &e);
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("{e:#}") })),
                )
                    .into_response()
            }
        },
        "generate-strategies" => {
            let domain = body.get("domain").and_then(Value::as_str).unwrap_or("");
            let scan = body
                .get("scan")
                .and_then(Value::as_str)
                .unwrap_or("veryquick");
            match crate::zapret::autotune::start_for_domain(&st, domain, scan) {
                Ok(()) => Json(json!({
                    "ok": true,
                    "message": "генерация стратегий запущена (подтвердите UAC)",
                    "status": crate::zapret::status_json(&st),
                }))
                .into_response(),
                Err(e) => {
                    crate::zapret::record_error(&st, &e);
                    (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": format!("{e:#}") })),
                    )
                        .into_response()
                }
            }
        }
        "apply-strategy" => {
            let Some(index) = body
                .get("index")
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
            else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "нужен index стратегии" })),
                )
                    .into_response();
            };
            match crate::zapret::autotune::apply_candidate(&st, index) {
                Ok(()) => Json(json!({
                    "ok": true,
                    "message": "установщик стратегии запущен (подтвердите UAC)",
                    "status": crate::zapret::status_json(&st),
                }))
                .into_response(),
                Err(e) => {
                    crate::zapret::record_error(&st, &e);
                    (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": format!("{e:#}") })),
                    )
                        .into_response()
                }
            }
        }
        "save-strategy" => {
            let Some(index) = body
                .get("index")
                .and_then(Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
            else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "нужен index стратегии" })),
                )
                    .into_response();
            };
            match crate::zapret::autotune::save_candidate(&st, index) {
                Ok(preset) => Json(json!({
                    "ok": true,
                    "preset": preset,
                    "status": crate::zapret::status_json(&st),
                }))
                .into_response(),
                Err(e) => {
                    crate::zapret::record_error(&st, &e);
                    (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": format!("{e:#}") })),
                    )
                        .into_response()
                }
            }
        }
        "catalog-status" => Json(crate::zapret::status_json(&st)).into_response(),
        "catalog-refresh" => {
            match crate::zapret::catalog::refresh(&st).await {
                Ok(report) => Json(json!({
                    "ok": true,
                    "report": report,
                    "status": crate::zapret::status_json(&st),
                }))
                .into_response(),
                Err(e) => (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("{e:#}") })),
                )
                    .into_response(),
            }
        }
        "catalog-download" => {
            let id = body.get("id").and_then(Value::as_str).unwrap_or("");
            if id.is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "нужен id элемента каталога" })),
                )
                    .into_response();
            }
            match crate::zapret::catalog::download(&st, id).await {
                Ok(count) => Json(json!({
                    "ok": true,
                    "downloaded": count,
                    "status": crate::zapret::status_json(&st),
                }))
                .into_response(),
                Err(e) => (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("{e:#}") })),
                )
                    .into_response(),
            }
        }
        "catalog-download-all" => {
            match crate::zapret::catalog::download_all_strategies(&st).await {
                Ok(report) => Json(json!({
                    "ok": true,
                    "report": report,
                    "status": crate::zapret::status_json(&st),
                }))
                .into_response(),
                Err(e) => (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("{e:#}") })),
                )
                    .into_response(),
            }
        }
        "catalog-apply" => {
            let id = body.get("id").and_then(Value::as_str).unwrap_or("");
            if id.is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "нужен id стратегии" })),
                )
                    .into_response();
            }
            match crate::zapret::catalog::apply(&st, id).await {
                Ok(()) => Json(json!({
                    "ok": true,
                    "message": "установщик стратегии каталога запущен (подтвердите UAC)",
                    "status": crate::zapret::status_json(&st),
                }))
                .into_response(),
                Err(e) => {
                    crate::zapret::record_error(&st, &e);
                    (
                        StatusCode::BAD_REQUEST,
                        Json(json!({ "error": format!("{e:#}") })),
                    )
                        .into_response()
                }
            }
        }
        "catalog-preview" => {
            let id = body.get("id").and_then(Value::as_str).unwrap_or("");
            match crate::zapret::catalog::preview_json(&st, id) {
                Ok(preview) => Json(json!({ "ok": true, "preview": preview })).into_response(),
                Err(e) => (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("{e:#}") })),
                )
                    .into_response(),
            }
        }
        "catalog-test" => {
            let domains: Vec<String> = body
                .get("domains")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(ToString::to_string)
                        .collect()
                })
                .unwrap_or_default();
            match crate::zapret::catalog::start_test(&st, &domains).await {
                Ok(()) => Json(json!({
                    "ok": true,
                    "message": "проверка стратегий запущена (подтвердите UAC)",
                    "status": crate::zapret::status_json(&st),
                }))
                .into_response(),
                Err(e) => (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("{e:#}") })),
                )
                    .into_response(),
            }
        }
        "catalog-test-reset" => {
            let force = body
                .get("force")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            match crate::zapret::catalog::reset_test(&st, force) {
                Ok(()) => Json(json!({
                    "ok": true,
                    "status": crate::zapret::status_json(&st),
                }))
                .into_response(),
                Err(e) => (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("{e:#}") })),
                )
                    .into_response(),
            }
        }
        "catalog-select" => {
            let lists: Vec<String> = body
                .get("lists")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(ToString::to_string)
                        .collect()
                })
                .unwrap_or_default();
            let ipsets: Vec<String> = body
                .get("ipsets")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(ToString::to_string)
                        .collect()
                })
                .unwrap_or_default();
            match crate::zapret::catalog::select(&st, &lists, &ipsets) {
                Ok(()) => Json(json!({
                    "ok": true,
                    "status": crate::zapret::status_json(&st),
                }))
                .into_response(),
                Err(e) => (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("{e:#}") })),
                )
                    .into_response(),
            }
        }
        "refresh" => Json(crate::zapret::status_json(&st)).into_response(),
        _ => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "action должен быть install|remove|start|stop|set-preset|open-folder|auto-tune|generate-strategies|apply-strategy|save-strategy|catalog-status|catalog-refresh|catalog-download|catalog-download-all|catalog-preview|catalog-test|catalog-test-reset|catalog-apply|catalog-select|probe|refresh"
            })),
        )
            .into_response(),
    }
}

async fn wg_configs_get(State(st): State<Arc<AppState>>) -> Response {
    match crate::engines::wg_configs::list(&st.dir) {
        Ok(configs) => Json(json!({ "configs": configs })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn wg_configs_post(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let action = body
        .get("action")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let slug = body
        .get("slug")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    match action {
        "add" => {
            let name = body
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .trim();
            let content = body
                .get("content")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            if name.is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "нужно имя конфига" })),
                )
                    .into_response();
            }
            match crate::engines::wg_configs::add(&st.dir, name, content) {
                Ok(slug) => {
                    let active = crate::engines::wg_configs::list(&st.dir)
                        .ok()
                        .and_then(|list| list.into_iter().find(|item| item.active));
                    if active
                        .as_ref()
                        .map(|item| item.slug == slug)
                        .unwrap_or(false)
                    {
                        st.wg.restart();
                    }
                    wlog!("WireGuard: добавлен конфиг «{name}»");
                    Json(json!({ "ok": true, "slug": slug })).into_response()
                }
                Err(e) => (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("{e:#}") })),
                )
                    .into_response(),
            }
        }
        "use" => match crate::engines::wg_configs::activate(&st.dir, slug) {
            Ok(()) => {
                st.wg.restart();
                crate::engines::wg_configs::log_active(&st.dir, slug);
                Json(json!({ "ok": true })).into_response()
            }
            Err(e) => (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("{e:#}") })),
            )
                .into_response(),
        },
        "remove" => match crate::engines::wg_configs::remove(&st.dir, slug) {
            Ok((removed_active, active)) => {
                if removed_active {
                    if active.is_some() {
                        st.wg.restart();
                    } else {
                        st.wg.stop();
                    }
                }
                wlog!("WireGuard: конфиг «{slug}» удалён");
                Json(json!({ "ok": true, "active": active })).into_response()
            }
            Err(e) => (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("{e:#}") })),
            )
                .into_response(),
        },
        "test" => match crate::engines::wg_configs::test(&st.dir, slug).await {
            Ok((ms, ip)) => Json(json!({ "ok": true, "ms": ms, "ip": ip })).into_response(),
            Err(e) => (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("{e:#}") })),
            )
                .into_response(),
        },
        _ => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "action должен быть add|use|remove|test" })),
        )
            .into_response(),
    }
}

async fn wg_config(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let action = body.get("action").and_then(|v| v.as_str()).unwrap_or("set");
    if action == "clear" {
        let active = crate::engines::wg_configs::list(&st.dir)
            .ok()
            .and_then(|list| list.into_iter().find(|item| item.active))
            .map(|item| item.slug);
        if let Some(slug) = active {
            let _ = crate::engines::wg_configs::remove(&st.dir, &slug);
        }
        let _ = std::fs::remove_file(st.dir.join("wg.conf"));
        let _ = std::fs::remove_file(st.dir.join("wireproxy.conf"));
        st.wg.stop();
        wlog!("wg.conf: удалён пользователем");
        return Json(json!({ "ok": true, "configured": false })).into_response();
    }
    let content = body.get("content").and_then(|v| v.as_str()).unwrap_or("");
    match crate::engines::wg_configs::add(&st.dir, "main", content)
        .and_then(|slug| crate::engines::wg_configs::activate(&st.dir, &slug).map(|_| slug))
    {
        Ok(slug) => {
            st.wg.restart();
            let endpoint = crate::engines::wireproxy::endpoint(&st.dir).unwrap_or_default();
            wlog!("wg.conf: сохранён (Endpoint {endpoint})");
            Json(json!({ "ok": true, "configured": true, "slug": slug, "endpoint": endpoint }))
                .into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("{e:#}") })),
        )
            .into_response(),
    }
}

async fn docs() -> Json<Value> {
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "readme": include_str!("../../README.md"),
        "readmeMac": include_str!("../../README-MAC.md"),
        "plugins": include_str!("../../PLUGINS.md"),
        "telegram": include_str!("../../TELEGRAM.md"),
        "zapret": include_str!("../../ZAPRET.md"),
    }))
}

fn version_tuple(value: &str) -> (u64, u64, u64) {
    let mut parts = value.trim_start_matches('v').split('.').map(|part| {
        part.chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse::<u64>()
            .unwrap_or(0)
    });
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}

/// Проверка последнего релиза на GitHub; ссылка ведёт на готовый бинарник.
async fn update_check() -> Response {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent(format!("wireproxy-control/{}", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(client) => client,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": format!("{e:#}") })),
            )
                .into_response()
        }
    };
    let response = client
        .get("https://api.github.com/repos/devcustrom/vless-pac-proxy/releases/latest")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": format!("GitHub недоступен: {e}") })),
            )
                .into_response()
        }
    };
    if !response.status().is_success() {
        return (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": format!("GitHub вернул {}", response.status()) })),
        )
            .into_response();
    }
    let data: Value = match response.json().await {
        Ok(data) => data,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": format!("некорректный ответ GitHub: {e}") })),
            )
                .into_response()
        }
    };
    let latest = data
        .get("tag_name")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim_start_matches('v')
        .to_string();
    if latest.is_empty() {
        return (
            StatusCode::BAD_GATEWAY,
            Json(json!({ "error": "в релизе нет tag_name" })),
        )
            .into_response();
    }
    let asset_name = if cfg!(windows) {
        "wireproxy-control-windows-x64.exe"
    } else if cfg!(target_os = "macos") {
        "wireproxy-control-macos-universal"
    } else {
        ""
    };
    let assets = data
        .get("assets")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let asset = assets
        .iter()
        .find(|item| item.get("name").and_then(|value| value.as_str()) == Some(asset_name));
    let url = asset
        .and_then(|item| item.get("browser_download_url"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let name = asset
        .and_then(|item| item.get("name"))
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let size = asset
        .and_then(|item| item.get("size"))
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let current = env!("CARGO_PKG_VERSION");
    let available = version_tuple(&latest) > version_tuple(current);
    Json(json!({
        "current": current,
        "latest": latest,
        "available": available,
        "url": url,
        "asset": name,
        "size": size,
        "publishedAt": data.get("published_at"),
        "page": data.get("html_url"),
    }))
    .into_response()
}

async fn update_install(State(st): State<Arc<AppState>>) -> Response {
    if !crate::update::begin_install() {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": "обновление уже устанавливается" })),
        )
            .into_response();
    }
    let result = async {
        let info = crate::update::latest().await?;
        if !info.available() {
            anyhow::bail!("установлена последняя версия");
        }
        let downloaded = crate::update::download(&st.dir, &info).await?;
        crate::update::spawn_installer(&st, &downloaded)?;
        Ok::<crate::update::ReleaseInfo, anyhow::Error>(info)
    }
    .await;
    match result {
        Ok(info) => {
            let st = st.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(1500)).await;
                cleanup_for_update(&st);
                tokio::time::sleep(Duration::from_millis(700)).await;
                std::process::exit(0);
            });
            Json(json!({
                "ok": true,
                "version": info.version,
                "restarting": true,
            }))
            .into_response()
        }
        Err(e) => {
            crate::update::end_install();
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("{e:#}") })),
            )
                .into_response()
        }
    }
}

async fn backup_download(State(st): State<Arc<AppState>>) -> Response {
    let result = tokio::task::spawn_blocking(move || crate::backup::create(&st)).await;
    match result {
        Ok(Ok(bytes)) => {
            let name = format!(
                "wireproxy-backup-{}.zip",
                chrono::Local::now().format("%Y%m%d-%H%M%S")
            );
            (
                [
                    (header::CONTENT_TYPE, "application/zip".to_string()),
                    (
                        header::CONTENT_DISPOSITION,
                        format!("attachment; filename=\"{name}\""),
                    ),
                ],
                bytes,
            )
                .into_response()
        }
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("{e:#}") })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn backup_restore(State(st): State<Arc<AppState>>, body: Bytes) -> Response {
    if body.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "пустой архив" })),
        )
            .into_response();
    }
    let path = crate::backup::restore_zip_path(&st.dir);
    if let Err(e) = std::fs::write(&path, &body) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response();
    }
    if let Err(e) = crate::backup::spawn_restore(&st, &path) {
        let _ = std::fs::remove_file(&path);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("{e:#}") })),
        )
            .into_response();
    }
    let st = st.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1500)).await;
        cleanup_for_update(&st);
        tokio::time::sleep(Duration::from_millis(700)).await;
        std::process::exit(0);
    });
    Json(json!({ "ok": true, "restarting": true })).into_response()
}

async fn engines_ensure(State(st): State<Arc<AppState>>) -> Response {
    let st = st.clone();
    tokio::spawn(async move {
        for kind in [EngineKind::Wireproxy, EngineKind::Xray] {
            if let Err(e) = crate::engines::downloads::ensure(&st.dir, kind, Some(st.clone())).await
            {
                wlog!("движки: {} — {e:#}", kind.label());
            }
        }
        start_enabled_engines(&st);
    });
    Json(json!({ "ok": true, "message": "загрузка началась" })).into_response()
}

async fn trust_certs_handler(State(st): State<Arc<AppState>>) -> Response {
    match crate::local_tls::trust_certs(&st.dir) {
        Ok(()) => {
            let script = st.dir.join(crate::local_tls::trust_script_name());
            match crate::shell::run_elevated(&script) {
                Ok(()) => Json(json!({ "ok": true })).into_response(),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": format!("{e:#}") })),
                )
                    .into_response(),
            }
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("{e:#}") })),
        )
            .into_response(),
    }
}

// ---------------- guards ----------------

async fn guard(State(st): State<Arc<AppState>>, req: Request, next: Next) -> Response {
    let host = req
        .headers()
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut allowed = host == format!("127.0.0.1:{}", st.panel_port)
        || host == format!("localhost:{}", st.panel_port);
    if !allowed {
        // домены, указывающие на панель (из плагина локальных домменов)
        let domains = read_local_domains(&st);
        let https_port = st
            .settings
            .lock()
            .map(|settings| settings.local_https_port)
            .unwrap_or(443);
        allowed = domains.iter().any(|d| {
            d.port == st.panel_port
                && (host == d.domain
                    || host == format!("{}:{}", d.domain, st.panel_port)
                    || host == format!("{}:{}", d.domain, https_port))
        });
    }
    if !allowed {
        return (StatusCode::FORBIDDEN, "forbidden host").into_response();
    }
    let needs_token = req.method() != Method::GET && req.method() != Method::HEAD;
    if needs_token {
        let ok = req
            .headers()
            .get("x-wpc")
            .and_then(|v| v.to_str().ok())
            .map(|v| v == "1")
            .unwrap_or(false);
        if !ok {
            return (StatusCode::FORBIDDEN, "missing x-wpc header").into_response();
        }
    }
    next.run(req).await
}

// ---------------- handlers ----------------

async fn index() -> Response {
    serve_ui("index.html")
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn serve_ui(path: &str) -> Response {
    let Some(file) = UI.get_file(path) else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    let cache = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    (
        [
            (header::CONTENT_TYPE, content_type(path)),
            (header::CACHE_CONTROL, cache),
        ],
        file.contents().to_vec(),
    )
        .into_response()
}

/// Отдаёт собранный фронтенд; неизвестные пути — index.html (SPA).
async fn ui_fallback(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if path.starts_with("api/") {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    if path.is_empty() {
        return serve_ui("index.html");
    }
    if UI.get_file(path).is_some() {
        return serve_ui(path);
    }
    serve_ui("index.html")
}

async fn pac(State(st): State<Arc<AppState>>) -> Response {
    let path = st.dir.join("proxy.pac");
    let body = std::fs::read_to_string(path).unwrap_or_else(|_| crate::lists::pac_content());
    (
        StatusCode::OK,
        [
            ("Content-Type", "application/x-ns-proxy-autoconfig"),
            ("Cache-Control", "no-cache"),
        ],
        body,
    )
        .into_response()
}

async fn status(State(st): State<Arc<AppState>>) -> Json<Value> {
    let settings = st.settings_snapshot();
    let health = st.health.lock().map(|h| h.clone()).unwrap_or_default();
    let (lists_counts, last_update, update_error, refreshing) = {
        let l = st.lists.lock().unwrap();
        (
            (l.proxied.len(), l.direct.len()),
            l.last_update,
            l.error.clone(),
            l.refreshing,
        )
    };
    let extra = read_domains(&st.dir, "extra-domains.txt");
    let direct_domains = read_domains(&st.dir, "direct-domains.txt");
    let vpn_only = read_domains(&st.dir, "vpn-only-domains.txt");
    let (auto_routes, auto_checking) = st
        .auto_routes
        .lock()
        .map(|routes| {
            let mut map = serde_json::Map::new();
            for (domain, decision) in &routes.routes {
                if extra.iter().any(|item| item == domain) {
                    if let Ok(value) = serde_json::to_value(decision) {
                        map.insert(domain.clone(), value);
                    }
                }
            }
            (Value::Object(map), routes.checking.len())
        })
        .unwrap_or((Value::Object(serde_json::Map::new()), 0));
    let subscriptions = read_subs(&st.dir);
    let (local_domains, local_http_err, local_https_err) = {
        let domains = st
            .local_domains
            .lock()
            .map(|d| d.clone())
            .unwrap_or_default();
        let http_err = st.local_http_error.lock().ok().and_then(|e| e.clone());
        let https_err = st.local_https_error.lock().ok().and_then(|e| e.clone());
        (domains, http_err, https_err)
    };
    let sys_proxy = crate::systemproxy::get_system_proxy().unwrap_or_default();
    let checks: Vec<CheckEntry> = st.store.checks(40).unwrap_or_default();
    let routing = st.routing_snapshot();
    let (pool_view, pool_testing, pool_updated, pool_error, next_pool_refresh) = {
        let p = st.pool.lock().unwrap();
        let view: Vec<Value> = p
            .entries
            .iter()
            .take(60)
            .enumerate()
            .map(|(i, e)| {
                json!({
                    "i": i,
                    "name": e.name,
                    "protocol": e.protocol,
                    "address": e.address,
                    "port": e.port,
                    "ok": e.ok,
                    "ms": e.ms,
                    "active": p.active_link.as_deref() == Some(e.link.as_str())
                })
            })
            .collect();
        let next = match (p.last_attempt_at, settings.auto_pool_refresh_hours) {
            (Some(t), h) if h > 0.0 => {
                Some((t + chrono::Duration::seconds((h * 3600.0) as i64)).to_rfc3339())
            }
            _ => None,
        };
        (
            Value::Array(view),
            p.testing,
            p.updated_at.map(|t| t.to_rfc3339()),
            p.error.clone(),
            next,
        )
    };
    let (visited, visited_total) = {
        let cutoff = chrono::Utc::now().timestamp() - 86_400;
        let rows = st.store.visited(cutoff, 300).unwrap_or_default();
        let total = rows.len();
        let view: Vec<Value> = rows
            .into_iter()
            .map(|row| {
                let route = routing.route_for(&row.domain).map(|channel| match channel {
                    crate::routing::Channel::Wireguard => "wireguard",
                    crate::routing::Channel::Vless => "vless",
                });
                let auto = routing.auto_for(&row.domain);
                let auto_route = auto.map(|choice| match choice.channel {
                    crate::routing::AutoChannel::Direct => "direct",
                    crate::routing::AutoChannel::Wireguard => "wireguard",
                    crate::routing::AutoChannel::Vless => "vless",
                });
                let route_source = if routing.is_direct_override(&row.domain) {
                    "direct"
                } else if routing.is_vpn_only(&row.domain) {
                    "vpn-only"
                } else if auto.is_some() {
                    "auto"
                } else if route.is_some() {
                    "default"
                } else {
                    "direct"
                };
                json!({
                    "domain": row.domain,
                    "count": row.count,
                    "last": row.last,
                    "route": route,
                    "routeSource": route_source,
                    "autoRoute": auto_route,
                    "inAuto": extra.iter().any(|item| item == &row.domain),
                    "checked": row.checked,
                })
            })
            .collect();
        (view, total)
    };
    let engines_ready = crate::engines::find_engine(EngineKind::Wireproxy).is_some()
        && crate::engines::find_engine(EngineKind::Xray).is_some();
    let downloads = st.downloads.lock().map(|d| d.clone()).unwrap_or_default();

    Json(json!({
        "vpnUp": st.wg.is_running(),
        "wgConfigured": crate::engines::wireproxy::is_configured(&st.dir),
        "wgEndpoint": crate::engines::wireproxy::endpoint(&st.dir),
        "proxyRunning": st.wg.is_running(),
        "wgUp": health.wg_ok,
        "exitIp": health.exit_ip,
        "xrayRunning": st.xr.is_running(),
        "vlessPortUp": st.xr.is_running(),
        "vlessConfigured": crate::engines::xray::has_config(&st.dir),
        "vlessUp": health.vless_ok,
        "exitIpVless": health.exit_ip_vless,
        "pool": pool_view,
        "poolTesting": pool_testing,
        "poolUpdatedAt": pool_updated,
        "poolError": pool_error,
        "autoRotate": settings.auto_rotate,
        "autoPoolRefreshHours": settings.auto_pool_refresh_hours,
        "directFirst": settings.direct_first,
        "autoCheckSites": settings.auto_check_sites,
        "autoAddUnavailable": settings.auto_add_unavailable,
        "autoDirectPrivate": settings.auto_direct_private,
        "autoConfigRunning": st
            .config_auto_running
            .load(std::sync::atomic::Ordering::SeqCst),
        "nextPoolRefreshAt": next_pool_refresh,
        "subscriptions": subscriptions,
        "localProxy": {
            "port": settings.local_proxy_port,
            "error": local_http_err,
            "httpsPort": settings.local_https_port,
            "httpsError": local_https_err,
            "caPath": "certs/ca.crt",
            "trustScript": crate::local_tls::trust_script_name(),
            "hostsScript": crate::local_hosts::hosts_script_name(),
            "panelDomain": local_domains
                .iter()
                .find(|d| d.port == st.panel_port)
                .or_else(|| local_domains.iter().find(|d| d.domain == "panel.localhost"))
                .map(|d| d.domain.clone()),
            "domains": local_domains.iter().map(|d| json!({ "domain": d.domain, "port": d.port })).collect::<Vec<_>>(),
        },
        "vpnDefault": settings.vpn_default,
        "version": env!("CARGO_PKG_VERSION"),
        "hwid": settings.hwid,
        "onboardingDone": settings.onboarding_done,
        "simpleUi": settings.simple_ui,
        "simple": crate::simple::status(&st),
        "plugins": {
            "localDomains": settings.local_domains_enabled,
            "telegram": settings.telegram_enabled,
            "zapret": settings.zapret_enabled && crate::zapret::supported(),
        },
        "telegram": crate::telegram::status::status_json(&st),
        "zapret": crate::zapret::status_json(&st),
        "channels": {
            "wireguard": settings.wireguard_enabled,
            "xray": settings.xray_enabled,
        },
        "autostart": crate::autostart::installed(),
        "caTrusted": crate::trust_status::is_trusted(&st.dir),
        "enginesReady": engines_ready,
        "engineDownloads": downloads,
        "autoRoutes": auto_routes,
        "autoChecking": auto_checking,
        "proxied": lists_counts.0,
        "direct": lists_counts.1,
        "lastUpdate": last_update,
        "updateError": update_error,
        "refreshing": refreshing,
        "mode": crate::lists::MODE,
        "systemProxy": sys_proxy,
        "smartProxyReady": st
            .smart_proxy_ready
            .load(std::sync::atomic::Ordering::SeqCst),
        "extra": extra,
        "directDomains": direct_domains,
        "vpnOnly": vpn_only,
        "checks": checks,
        "visited": visited,
        "visitedTotal": visited_total,
    }))
}

async fn extra(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let action = body.get("action").and_then(|v| v.as_str()).unwrap_or("");
    let Some(domain) = body
        .get("domain")
        .and_then(|v| v.as_str())
        .and_then(crate::lists::validate_hostname)
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "некорректный домен" })),
        )
            .into_response();
    };
    let mut extra = read_domains(&st.dir, "extra-domains.txt");
    let mut direct = read_domains(&st.dir, "direct-domains.txt");
    let mut vpn_only = read_domains(&st.dir, "vpn-only-domains.txt");
    if action == "direct-remove" {
        let before = direct.len();
        direct.retain(|item| *item != domain);
        if direct.len() != before {
            if let Err(e) = write_domains(&st.dir, "direct-domains.txt", &direct) {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
                    .into_response();
            }
            spawn_refresh(&st);
        }
        return Json(json!({
            "ok": true,
            "direct": read_domains(&st.dir, "direct-domains.txt"),
        }))
        .into_response();
    }
    if action == "vpn-only-add" || action == "vpn-only-remove" {
        if action == "vpn-only-add" {
            if !extra.contains(&domain) {
                extra.push(domain.clone());
            }
            direct.retain(|item| *item != domain);
            if !vpn_only.contains(&domain) {
                vpn_only.push(domain.clone());
            }
        } else {
            vpn_only.retain(|item| *item != domain);
        }
        if let Err(e) = write_domains(&st.dir, "extra-domains.txt", &extra) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response();
        }
        if let Err(e) = write_domains(&st.dir, "direct-domains.txt", &direct) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response();
        }
        if let Err(e) = write_domains(&st.dir, "vpn-only-domains.txt", &vpn_only) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response();
        }
        crate::zapret::sync_domains(&st, &extra);
        crate::route_auto::spawn_check_one(st.clone(), domain.clone());
        spawn_refresh(&st);
        return Json(json!({
            "ok": true,
            "extra": read_domains(&st.dir, "extra-domains.txt"),
            "direct": read_domains(&st.dir, "direct-domains.txt"),
            "vpnOnly": read_domains(&st.dir, "vpn-only-domains.txt"),
        }))
        .into_response();
    }
    // add — в авто-список; remove — только убрать из авто;
    // direct-add — принудительно напрямую (и убрать из авто).
    let (add, direct_add) = match action {
        "add" => (true, false),
        "remove" => (false, false),
        "direct-add" => (false, true),
        "route" => match body.get("enabled").and_then(Value::as_bool) {
            Some(true) => (true, false),
            Some(false) => (false, true),
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "для route нужен enabled" })),
                )
                    .into_response();
            }
        },
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "action должен быть add|remove|direct-add|direct-remove|vpn-only-add|vpn-only-remove|route"
                })),
            )
                .into_response();
        }
    };
    if add {
        if !extra.contains(&domain) {
            extra.push(domain.clone());
        }
        direct.retain(|item| *item != domain);
    } else if direct_add {
        extra.retain(|item| *item != domain);
        vpn_only.retain(|item| *item != domain);
        if !direct.contains(&domain) {
            direct.push(domain.clone());
        }
    } else {
        extra.retain(|item| *item != domain);
        vpn_only.retain(|item| *item != domain);
    }
    if let Err(e) = write_domains(&st.dir, "extra-domains.txt", &extra) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response();
    }
    if let Err(e) = write_domains(&st.dir, "direct-domains.txt", &direct) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response();
    }
    if let Err(e) = write_domains(&st.dir, "vpn-only-domains.txt", &vpn_only) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response();
    }
    crate::zapret::sync_domains(&st, &extra);
    if add {
        crate::route_auto::spawn_check_one(st.clone(), domain.clone());
    } else {
        crate::route_auto::remove_decision(&st, &domain);
    }
    spawn_refresh(&st);
    Json(json!({
        "ok": true,
        "enabled": add,
        "extra": read_domains(&st.dir, "extra-domains.txt"),
        "direct": read_domains(&st.dir, "direct-domains.txt"),
        "vpnOnly": read_domains(&st.dir, "vpn-only-domains.txt"),
    }))
    .into_response()
}

async fn resolve_xray_link(
    st: &Arc<AppState>,
    raw: &str,
) -> anyhow::Result<(crate::vless::link::ParsedLink, bool)> {
    let trimmed = raw.trim_start_matches('\u{feff}').trim();
    if trimmed.is_empty() {
        anyhow::bail!("вставьте ссылку или JSON Outline");
    }
    if crate::vless::link::is_outline_config_url(trimmed) {
        Ok((
            crate::vless::pool::resolve_outline_dynamic(st, trimmed).await?,
            true,
        ))
    } else if trimmed.starts_with('{') {
        Ok((
            crate::vless::link::parse_outline_payload(trimmed, "")?,
            false,
        ))
    } else {
        Ok((crate::vless::link::parse_any(trimmed)?, false))
    }
}

fn clear_pool_active(st: &Arc<AppState>) {
    if let Ok(mut pool) = st.pool.lock() {
        pool.active_link = None;
    }
    crate::vless::pool::save_pool_state(st);
}

/// Перезапуск Xray только при включённом канале: добавление конфига не должно
/// запускать движок, если пользователь выключил Xray.
fn restart_xray_if_enabled(st: &Arc<AppState>) {
    if st.settings_snapshot().xray_enabled {
        st.xr.restart();
    }
}

async fn xray_configs_get(State(st): State<Arc<AppState>>) -> Response {
    match crate::vless::configs::list(&st.dir) {
        Ok(configs) => Json(json!({ "configs": configs })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("{e:#}") })),
        )
            .into_response(),
    }
}

async fn xray_configs_post(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let action = body
        .get("action")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let slug = body
        .get("slug")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    match action {
        "add" => {
            let name = body
                .get("name")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let link = body
                .get("link")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let result = async {
                let (parsed, dynamic) = resolve_xray_link(&st, link).await?;
                let slug = crate::vless::configs::add_parsed(&st.dir, name, &parsed, dynamic)?;
                let active =
                    crate::vless::configs::active_slug(&st.dir).as_deref() == Some(slug.as_str());
                Ok::<_, anyhow::Error>((slug, active))
            }
            .await;
            match result {
                Ok((slug, active)) => {
                    if active {
                        clear_pool_active(&st);
                        restart_xray_if_enabled(&st);
                        crate::vless::configs::log_active(&st.dir, &slug);
                    }
                    Json(json!({ "ok": true, "slug": slug, "active": active })).into_response()
                }
                Err(e) => (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("{e:#}") })),
                )
                    .into_response(),
            }
        }
        "use" => match crate::vless::configs::activate(&st.dir, slug) {
            Ok(()) => {
                clear_pool_active(&st);
                restart_xray_if_enabled(&st);
                crate::vless::configs::log_active(&st.dir, slug);
                Json(json!({ "ok": true })).into_response()
            }
            Err(e) => (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("{e:#}") })),
            )
                .into_response(),
        },
        "remove" => match crate::vless::configs::remove(&st.dir, slug) {
            Ok((was_active, next)) => {
                if slug == crate::vless::configs::POOL_SLUG {
                    clear_pool_active(&st);
                }
                if was_active {
                    if next.is_some() {
                        restart_xray_if_enabled(&st);
                    } else {
                        st.xr.stop();
                    }
                }
                wlog!("Xray: конфиг «{slug}» удалён");
                Json(json!({ "ok": true, "active": next })).into_response()
            }
            Err(e) => (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("{e:#}") })),
            )
                .into_response(),
        },
        "test" => match crate::vless::configs::test(&st.dir, slug).await {
            Ok((ms, ip)) => Json(json!({ "ok": true, "ms": ms, "ip": ip })).into_response(),
            Err(e) => (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": format!("{e:#}") })),
            )
                .into_response(),
        },
        _ => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "action должен быть add|use|remove|test" })),
        )
            .into_response(),
    }
}

async fn vless(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let action = body.get("action").and_then(|v| v.as_str()).unwrap_or("");
    match action {
        "set-link" => {
            let link = body.get("link").and_then(|v| v.as_str()).unwrap_or("");
            let result = async {
                let (parsed, dynamic) = resolve_xray_link(&st, link).await?;
                let name = if parsed.name.trim().is_empty() {
                    parsed.address.clone()
                } else {
                    parsed.name.clone()
                };
                let slug = crate::vless::configs::add_parsed(&st.dir, &name, &parsed, dynamic)?;
                crate::vless::configs::activate(&st.dir, &slug)?;
                let info = crate::vless::link::link_info(&parsed);
                Ok::<_, anyhow::Error>((info, slug))
            }
            .await;
            match result {
                Ok((info, slug)) => {
                    clear_pool_active(&st);
                    restart_xray_if_enabled(&st);
                    crate::vless::configs::log_active(&st.dir, &slug);
                    Json(json!({ "ok": true, "info": info })).into_response()
                }
                Err(e) => (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": e.to_string() })),
                )
                    .into_response(),
            }
        }
        "remove-link" => {
            let result = if let Some(slug) = crate::vless::configs::active_slug(&st.dir) {
                crate::vless::configs::remove(&st.dir, &slug).map(|_| ())
            } else {
                for name in [
                    "xray-config.json",
                    "xray-config.json.bak",
                    "xray-config.json.tmp",
                ] {
                    let _ = std::fs::remove_file(st.dir.join(name));
                }
                Ok(())
            };
            match result {
                Ok(()) => {
                    clear_pool_active(&st);
                    if crate::vless::configs::active_slug(&st.dir).is_some() {
                        restart_xray_if_enabled(&st);
                    } else {
                        st.xr.stop();
                    }
                    wlog!("Xray-конфиг удалён пользователем");
                    Json(json!({
                        "ok": true,
                        "configured": crate::vless::link::has_config(&st.dir)
                    }))
                    .into_response()
                }
                Err(e) => (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": format!("{e:#}") })),
                )
                    .into_response(),
            }
        }
        "set-default" => {
            let value = body.get("value").and_then(|v| v.as_str()).unwrap_or("");
            if value != "wireguard" && value != "vless" {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "value должен быть wireguard|vless" })),
                )
                    .into_response();
            }
            let mut candidate = st.settings_snapshot();
            candidate.vpn_default = value.to_string();
            if let Err(e) = candidate.save(&st.dir) {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
                    .into_response();
            }
            *st.settings.lock().unwrap() = candidate.clone();
            Json(json!({ "ok": true, "vpnDefault": candidate.vpn_default })).into_response()
        }
        "restart" => {
            st.xr.restart();
            Json(json!({ "ok": true })).into_response()
        }
        _ => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "неизвестное действие" })),
        )
            .into_response(),
    }
}

async fn refresh_lists(State(st): State<Arc<AppState>>) -> Response {
    spawn_refresh(&st);
    (StatusCode::ACCEPTED, Json(json!({ "ok": true }))).into_response()
}

async fn system_proxy(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let Some(enabled) = body.get("enabled").and_then(|v| v.as_bool()) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "нужно enabled: true|false" })),
        )
            .into_response();
    };
    if enabled
        && !st
            .smart_proxy_ready
            .load(std::sync::atomic::Ordering::SeqCst)
    {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "умный прокси не слушает 127.0.0.1:8081 — порт занят?" })),
        )
            .into_response();
    }
    match crate::systemproxy::apply(&st, enabled) {
        Ok(state) => Json(json!({ "ok": true, "state": state })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

fn start_enabled_engines(state: &Arc<AppState>) {
    let settings = state.settings_snapshot();
    if settings.wireguard_enabled {
        state.wg.start();
    }
    if settings.xray_enabled {
        state.xr.start();
    }
}

async fn onboarding_done(State(st): State<Arc<AppState>>) -> Json<Value> {
    let mut candidate = st.settings_snapshot();
    candidate.onboarding_done = true;
    if let Err(e) = candidate.save(&st.dir) {
        wlog!("onboarding: сохранение настроек — {e:#}");
    } else {
        *st.settings.lock().unwrap() = candidate;
    }
    Json(json!({ "ok": true, "onboardingDone": true }))
}

async fn ui_mode(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let Some(simple) = body.get("simple").and_then(Value::as_bool) else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "нужно simple: true|false" })),
        )
            .into_response();
    };
    let mut settings = st.settings_snapshot();
    settings.simple_ui = simple;
    if let Err(e) = settings.save(&st.dir) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response();
    }
    *st.settings.lock().unwrap() = settings;
    Json(json!({ "ok": true, "simpleUi": simple })).into_response()
}

async fn simple_enable(State(st): State<Arc<AppState>>) -> Response {
    crate::simple::spawn_enable(st.clone());
    (
        StatusCode::ACCEPTED,
        Json(json!({ "ok": true, "simple": crate::simple::status(&st) })),
    )
        .into_response()
}

async fn simple_disable(State(st): State<Arc<AppState>>) -> Response {
    match crate::simple::disable(&st) {
        Ok(()) => Json(json!({
            "ok": true,
            "simple": crate::simple::status(&st),
        }))
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("{e:#}") })),
        )
            .into_response(),
    }
}

async fn channels(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let channel = body
        .get("channel")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if channel != "wireguard" && channel != "xray" {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "channel должен быть wireguard|xray" })),
        )
            .into_response();
    }
    let enabled = body.get("enabled").and_then(|value| value.as_bool());
    let restart = body
        .get("restart")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let current = st.settings_snapshot();
    let target = enabled.unwrap_or(if channel == "wireguard" {
        current.wireguard_enabled
    } else {
        current.xray_enabled
    });
    if enabled.is_some() {
        let mut candidate = st.settings_snapshot();
        if channel == "wireguard" {
            candidate.wireguard_enabled = target;
        } else {
            candidate.xray_enabled = target;
        }
        if let Err(e) = candidate.save(&st.dir) {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response();
        }
        *st.settings.lock().unwrap() = candidate;
    }
    let engine = if channel == "wireguard" {
        &st.wg
    } else {
        &st.xr
    };
    if enabled.is_some() {
        if target {
            engine.start();
        } else {
            engine.stop();
        }
        wlog!(
            "канал {channel}: {}",
            if target {
                "включён"
            } else {
                "выключен"
            }
        );
    }
    if restart && target {
        engine.restart();
        wlog!("канал {channel}: перезапуск");
    }
    if enabled.is_some() || restart {
        crate::route_auto::spawn_check_all(st.clone());
    }
    let settings = st.settings_snapshot();
    Json(json!({
        "ok": true,
        "channels": {
            "wireguard": settings.wireguard_enabled,
            "xray": settings.xray_enabled,
        }
    }))
    .into_response()
}

async fn auto(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let hours = body.get("autoPoolRefreshHours").and_then(|v| v.as_f64());
    if let Some(v) = hours {
        if !(0.0..=168.0).contains(&v) {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "autoPoolRefreshHours: 0..168" })),
            )
                .into_response();
        }
    }
    let mut s = st.settings.lock().unwrap();
    let mut enable_auto = false;
    if let Some(v) = body.get("autoRotate").and_then(|v| v.as_bool()) {
        s.auto_rotate = v;
        enable_auto = v;
    }
    if let Some(v) = body.get("directFirst").and_then(|v| v.as_bool()) {
        s.direct_first = v;
        if let Ok(mut cache) = st.direct_probe.lock() {
            cache.clear();
        }
    }
    if let Some(v) = hours {
        s.auto_pool_refresh_hours = v;
    }
    if let Some(v) = body.get("autoCheckSites").and_then(|v| v.as_bool()) {
        s.auto_check_sites = v;
    }
    if let Some(v) = body.get("autoAddUnavailable").and_then(|v| v.as_bool()) {
        s.auto_add_unavailable = v;
    }
    if let Some(v) = body.get("autoDirectPrivate").and_then(|v| v.as_bool()) {
        s.auto_direct_private = v;
    }
    if let Err(e) = s.save(&st.dir) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response();
    }
    let snapshot = s.clone();
    drop(s);
    if enable_auto {
        crate::config_auto::spawn_once(st.clone());
    }
    Json(json!({
        "ok": true,
        "autoRotate": snapshot.auto_rotate,
        "autoPoolRefreshHours": snapshot.auto_pool_refresh_hours,
        "directFirst": snapshot.direct_first,
        "autoCheckSites": snapshot.auto_check_sites,
        "autoAddUnavailable": snapshot.auto_add_unavailable,
        "autoDirectPrivate": snapshot.auto_direct_private,
    }))
    .into_response()
}

async fn local_domains_get(State(st): State<Arc<AppState>>) -> Json<Value> {
    let s = st.settings_snapshot();
    let domains = st
        .local_domains
        .lock()
        .map(|d| d.clone())
        .unwrap_or_default();
    let http_err = st.local_http_error.lock().ok().and_then(|e| e.clone());
    let https_err = st.local_https_error.lock().ok().and_then(|e| e.clone());
    Json(json!({
        "httpPort": s.local_proxy_port,
        "httpsPort": s.local_https_port,
        "error": http_err,
        "httpsError": https_err,
        "caPath": "certs/ca.crt",
        "trustScript": crate::local_tls::trust_script_name(),
        "hostsScript": crate::local_hosts::hosts_script_name(),
        "domains": domains.iter().map(|d| json!({ "domain": d.domain, "port": d.port })).collect::<Vec<_>>(),
    }))
}

async fn local_domains_post(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let action = body.get("action").and_then(|v| v.as_str()).unwrap_or("");
    let Some(domain) = body
        .get("domain")
        .and_then(|v| v.as_str())
        .and_then(crate::lists::validate_hostname)
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "некорректный домен" })),
        )
            .into_response();
    };
    let mut list = read_local_domains(&st);
    match action {
        "add" => {
            let port = body.get("port").and_then(|v| v.as_u64()).unwrap_or(0);
            if !(1..=65535).contains(&port) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "порт должен быть 1..65535" })),
                )
                    .into_response();
            }
            let settings = st.settings_snapshot();
            if crate::net::local_proxy::is_self_target(port as u16, &settings) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "нельзя направить домен на сам локальный прокси" })),
                )
                    .into_response();
            }
            list.retain(|d| d.domain != domain);
            list.push(crate::net::local_proxy::LocalDomain {
                domain,
                port: port as u16,
            });
        }
        "remove" => list.retain(|d| d.domain != domain),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "action должен быть add|remove" })),
            )
                .into_response();
        }
    }
    match write_local_domains(&st.dir, &list) {
        Ok(()) => {
            if let Ok(mut d) = st.local_domains.lock() {
                *d = list.clone();
            }
            reload_local_tls(&st).await;
            if crate::systemproxy::get_system_proxy()
                .map(|state| state.enabled)
                .unwrap_or(false)
            {
                let names: Vec<String> = list.iter().map(|item| item.domain.clone()).collect();
                if let Err(e) = crate::systemproxy::set_system_proxy(true, &names) {
                    wlog!("системный прокси: локальные домены в исключения — {e:#}");
                }
            }
            Json(json!({ "ok": true, "domains": list.iter().map(|d| json!({ "domain": d.domain, "port": d.port })).collect::<Vec<_>>() })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn reload_local_tls(st: &Arc<AppState>) {
    let names: Vec<String> = st
        .local_domains
        .lock()
        .map(|d| d.iter().map(|x| x.domain.clone()).collect())
        .unwrap_or_default();
    let holder = st.tls_config.lock().ok().and_then(|s| s.clone());
    let dir = st.dir.clone();
    let result = tokio::task::spawn_blocking(move || {
        if let Err(e) = crate::local_hosts::write_script(&dir, &names) {
            wlog!("локальные домены: скрипт hosts — {e:#}");
        }
        crate::local_tls::build_store(&dir, &names)
    })
    .await;
    match result {
        Ok(Ok(store)) => {
            let cfg = crate::local_tls::server_config(Arc::new(store));
            if let Some(h) = holder {
                *h.write().await = cfg;
            }
            if let Ok(mut e) = st.local_https_error.lock() {
                *e = None;
            }
        }
        Ok(Err(e)) => {
            if let Ok(mut x) = st.local_https_error.lock() {
                *x = Some(format!("{e:#}"));
            }
            wlog!("локальный HTTPS: сертификаты — {e:#}");
        }
        Err(e) => wlog!("локальный HTTPS: задача сборки сертификатов упала — {e}"),
    }
}

async fn local_domains_hosts(State(st): State<Arc<AppState>>) -> Response {
    let names: Vec<String> = st
        .local_domains
        .lock()
        .map(|list| list.iter().map(|item| item.domain.clone()).collect())
        .unwrap_or_default();
    match crate::local_hosts::write_script(&st.dir, &names) {
        Ok(path) => {
            Json(json!({ "ok": true, "script": path.display().to_string() })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("{e:#}") })),
        )
            .into_response(),
    }
}

async fn local_domains_hosts_apply(State(st): State<Arc<AppState>>) -> Response {
    let names: Vec<String> = st
        .local_domains
        .lock()
        .map(|list| list.iter().map(|item| item.domain.clone()).collect())
        .unwrap_or_default();
    match crate::local_hosts::write_script(&st.dir, &names)
        .and_then(|script| crate::shell::run_elevated(&script).map(|_| script))
    {
        Ok(script) => {
            Json(json!({ "ok": true, "script": script.display().to_string() })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("{e:#}") })),
        )
            .into_response(),
    }
}

async fn local_domains_hosts_edit(State(st): State<Arc<AppState>>) -> Response {
    match crate::local_hosts::edit_script(&st.dir)
        .and_then(|script| crate::shell::run_elevated_interactive(&script).map(|_| script))
    {
        Ok(script) => {
            Json(json!({ "ok": true, "script": script.display().to_string() })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("{e:#}") })),
        )
            .into_response(),
    }
}

async fn local_domains_probe(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let domain = body
        .get("domain")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    let target = st
        .local_domains
        .lock()
        .map(|d| d.iter().find(|x| x.domain == domain).cloned())
        .ok()
        .flatten();
    let Some(target) = target else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "домен не настроен" })),
        )
            .into_response();
    };
    let http_port = st.settings_snapshot().local_proxy_port;
    let started = std::time::Instant::now();
    let status = probe_local_domain(&domain, http_port).await;
    let ms = started.elapsed().as_millis() as u64;
    match status {
        Some(code) => Json(json!({
            "domain": domain,
            "port": target.port,
            "status": code,
            "ms": ms
        }))
        .into_response(),
        None => Json(json!({
            "domain": domain,
            "port": target.port,
            "status": 0,
            "ms": ms,
            "error": "локальный домен не ответил"
        }))
        .into_response(),
    }
}

async fn probe_local_domain(domain: &str, http_port: u16) -> Option<u16> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    let mut sock = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        TcpStream::connect(("127.0.0.1", http_port)),
    )
    .await
    .ok()?
    .ok()?;
    let request = format!("GET / HTTP/1.1\r\nHost: {domain}\r\nConnection: close\r\n\r\n");
    sock.write_all(request.as_bytes()).await.ok()?;
    let mut buf = vec![0u8; 256];
    let n = tokio::time::timeout(std::time::Duration::from_secs(5), sock.read(&mut buf))
        .await
        .ok()?
        .ok()?;
    let head = String::from_utf8_lossy(&buf[..n]).to_string();
    head.lines().next()?.split_whitespace().nth(1)?.parse().ok()
}

async fn local_ports(State(_st): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({ "ports": scan_local_ports() }))
}

fn scan_local_ports() -> Vec<u16> {
    use std::process::Command;
    let mut ports: Vec<u16> = Vec::new();
    let mut cmd = Command::new("netstat");
    cmd.args(["-ano", "-p", "TCP"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    if let Ok(output) = cmd.output() {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let mut parts = line.split_whitespace();
            if !parts
                .next()
                .map(|p| p.eq_ignore_ascii_case("TCP"))
                .unwrap_or(false)
            {
                continue;
            }
            let Some(local) = parts.next() else { continue };
            if parts.next().is_none() {
                continue;
            }
            let state = parts.next().unwrap_or("");
            if !state.eq_ignore_ascii_case("LISTENING") {
                continue;
            }
            let is_loopback = local.starts_with("127.0.0.1:") || local.starts_with("[::1]:");
            if !is_loopback {
                continue;
            }
            if let Some((_, port)) = local.rsplit_once(':') {
                if let Ok(p) = port.parse::<u16>() {
                    ports.push(p);
                }
            }
        }
    }
    ports.sort_unstable();
    ports.dedup();
    ports
}

async fn check(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let Some(domain) = body
        .get("domain")
        .and_then(|v| v.as_str())
        .and_then(crate::lists::validate_hostname)
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "некорректный домен" })),
        )
            .into_response();
    };
    let outcome = crate::site_check::check_domain_full(&st, &domain).await;
    let entry = &outcome.entry;
    let best_route = outcome
        .best
        .as_ref()
        .map(|decision| match decision.channel {
            crate::routing::AutoChannel::Direct => "direct",
            crate::routing::AutoChannel::Wireguard => "wireguard",
            crate::routing::AutoChannel::Vless => "vless",
        });
    let best_latency = outcome.best.as_ref().map(|decision| decision.latency_ms);
    Json(json!({
        "domain": entry.domain,
        "direct": entry.direct,
        "vpn": entry.vpn,
        "zapret": entry.zapret,
        "zapretRunning": entry.zapret_running,
        "zapretSupported": entry.zapret_supported,
        "verdict": entry.verdict,
        "wireguard": outcome.probes.wireguard,
        "vless": outcome.probes.vless,
        "bestRoute": best_route,
        "bestLatencyMs": best_latency,
        "inAuto": outcome.in_auto,
        "at": entry.at,
    }))
    .into_response()
}

async fn routes_check(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    if let Some(raw) = body.get("domain").and_then(|v| v.as_str()) {
        let Some(domain) = crate::lists::validate_hostname(raw) else {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "некорректный домен" })),
            )
                .into_response();
        };
        crate::route_auto::spawn_check_one(st.clone(), domain);
    } else {
        crate::route_auto::spawn_check_all(st.clone());
    }
    (StatusCode::ACCEPTED, Json(json!({ "ok": true }))).into_response()
}

async fn configs_auto(State(st): State<Arc<AppState>>) -> Response {
    crate::config_auto::spawn_once(st);
    (StatusCode::ACCEPTED, Json(json!({ "ok": true }))).into_response()
}

async fn pool_refresh(State(st): State<Arc<AppState>>) -> Response {
    if st.pool.lock().map(|p| p.testing).unwrap_or(false) {
        return (
            StatusCode::CONFLICT,
            Json(json!({ "error": "уже выполняется" })),
        )
            .into_response();
    }
    let st2 = st.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::vless::pool::refresh(&st2).await {
            wlog!("обновление пула: {e}");
        }
    });
    (StatusCode::ACCEPTED, Json(json!({ "ok": true }))).into_response()
}

async fn pool_auto(State(st): State<Arc<AppState>>) -> Response {
    match crate::vless::pool::auto_select_best(&st) {
        Ok(info) => Json(json!({
            "ok": true,
            "info": info,
            "provider": "pool",
        }))
        .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("{e:#}") })),
        )
            .into_response(),
    }
}

async fn pool_use(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let index = body
        .get("index")
        .and_then(|v| v.as_u64())
        .unwrap_or(u64::MAX) as usize;
    match crate::vless::pool::apply_entry_checked(&st, index).await {
        Ok(info) => Json(json!({ "ok": true, "info": info })).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn subscriptions(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let action = body.get("action").and_then(|v| v.as_str()).unwrap_or("");
    let url = body
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let mut list = crate::vless::pool::read_subscriptions(&st.dir);
    let mut imported = false;
    let mut imported_count = 0usize;
    match action {
        "add" => {
            if let Err(e) = crate::vless::pool::validate_subscription_url(&url).await {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": e.to_string() })),
                )
                    .into_response();
            }
            if !list.contains(&url) {
                list.push(url);
            }
        }
        "import-igareck" => {
            let groups: Vec<String> = body
                .get("groups")
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_str().map(str::to_string))
                        .collect()
                })
                .or_else(|| {
                    body.get("group")
                        .and_then(|value| value.as_str())
                        .map(|group| vec![group.to_string()])
                })
                .unwrap_or_default();
            let mut added = 0;
            for candidate in crate::vless::pool::igareck_group_urls(&groups) {
                let file_name = candidate.rsplit('/').next().unwrap_or("").to_lowercase();
                let duplicate = list.iter().any(|existing| {
                    existing
                        .rsplit('/')
                        .next()
                        .unwrap_or("")
                        .eq_ignore_ascii_case(&file_name)
                });
                if duplicate {
                    continue;
                }
                if crate::vless::pool::validate_subscription_url(&candidate)
                    .await
                    .is_ok()
                {
                    list.push(candidate);
                    added += 1;
                }
            }
            imported = true;
            imported_count = added;
            wlog!("подписки: импортировано из igareck — {added}");
        }
        "remove" => list.retain(|u| *u != url),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "action должен быть add|remove|import-igareck" })),
            )
                .into_response();
        }
    }
    if let Err(e) = crate::vless::pool::write_subscriptions(&st.dir, &list) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response();
    }
    if imported {
        let st = st.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::vless::pool::refresh(&st).await {
                wlog!("импорт igareck: проверка пула — {e}");
            }
        });
    }
    Json(json!({
        "ok": true,
        "added": imported_count,
        "subscriptions": crate::vless::pool::read_subscriptions(&st.dir)
    }))
    .into_response()
}

// ---------------- файловые помощники ----------------

pub fn read_domains(dir: &std::path::Path, name: &str) -> Vec<String> {
    crate::lists::read_domain_file(dir, name)
}

pub fn write_domains(dir: &std::path::Path, name: &str, list: &[String]) -> std::io::Result<()> {
    crate::lists::write_domain_file(dir, name, list)
}

fn read_subs(dir: &std::path::Path) -> Vec<String> {
    let Ok(path) = crate::paths::in_dir(dir, "subscriptions.txt") else {
        return Vec::new();
    };
    std::fs::read_to_string(path)
        .map(|t| {
            t.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .collect()
        })
        .unwrap_or_default()
}

fn read_local_domains(st: &Arc<AppState>) -> Vec<crate::net::local_proxy::LocalDomain> {
    let Ok(path) = crate::paths::in_dir(&st.dir, "local-domains.txt") else {
        return Vec::new();
    };
    std::fs::read_to_string(path)
        .map(|t| crate::net::local_proxy::parse_config(&t))
        .unwrap_or_default()
}

fn write_local_domains(
    dir: &std::path::Path,
    list: &[crate::net::local_proxy::LocalDomain],
) -> std::io::Result<()> {
    let path = crate::paths::in_dir(dir, "local-domains.txt")
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()))?;
    let mut text = String::from(
        "# Плагин локальных доменов: домен = порт (по одному в строке).\n\
         # *.localhost браузеры резолвят в 127.0.0.1 без правки hosts.\n\
         # Для «настоящих» имён запустите local-domains-hosts.bat от администратора.\n\n",
    );
    for d in list {
        text.push_str(&format!("{} = {}\n", d.domain, d.port));
    }
    std::fs::write(path, text)
}

fn spawn_refresh(st: &Arc<AppState>) {
    let st = st.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::lists::refresh(&st).await {
            wlog!("обновление списков: {e}");
        }
    });
}

// ---------------- health ----------------

fn spawn_health_probes(state: Arc<AppState>) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(3)).await;
        loop {
            probe_once(&state).await;
            let (wg_ok, vless_ok) = state.health_snapshot();
            let settings = state.settings_snapshot();
            let wg_needed =
                settings.wireguard_enabled && crate::engines::wireproxy::is_configured(&state.dir);
            let xr_needed = settings.xray_enabled && crate::engines::xray::has_config(&state.dir);
            let healthy = (!wg_needed || wg_ok) && (!xr_needed || vless_ok);
            // Пока канал не поднялся — проверяем чаще, чтобы панель не висела
            // красной минуту после старта.
            let delay = if healthy { 60 } else { 10 };
            tokio::time::sleep(Duration::from_secs(delay)).await;
        }
    });
}

async fn probe_once(state: &Arc<AppState>) {
    let settings = state.settings_snapshot();
    let wg_enabled = settings.wireguard_enabled;
    let xr_enabled = settings.xray_enabled;
    let wg_client = crate::state::probe_client(crate::vless::WIREPROXY_HTTP_PORT);
    let wg_ip = if wg_enabled {
        exit_ip(&wg_client).await
    } else {
        None
    };
    let xr_configured = xr_enabled && crate::engines::xray::has_config(&state.dir);
    let xr_client = crate::state::probe_client(crate::vless::XRAY_HTTP_PORT);
    let xr_ip = if xr_configured {
        exit_ip(&xr_client).await
    } else {
        None
    };
    let (wg_was, xr_was) = state
        .health
        .lock()
        .map(|h| (h.wg_ok, h.vless_ok))
        .unwrap_or((false, false));
    let (wg_now, xr_now) = if let Ok(mut h) = state.health.lock() {
        if !wg_enabled {
            h.wg_ok = false;
            h.exit_ip = None;
            h.wg_fail = 0;
        } else if let Some(ip) = &wg_ip {
            h.wg_ok = true;
            h.exit_ip = Some(ip.clone());
            h.wg_fail = 0;
        } else {
            h.wg_fail = h.wg_fail.saturating_add(1);
            if h.wg_fail >= 2 {
                h.wg_ok = false;
                h.exit_ip = None;
            }
        }
        if !xr_enabled {
            h.vless_ok = false;
            h.exit_ip_vless = None;
            h.vless_fail = 0;
        } else if let Some(ip) = &xr_ip {
            h.vless_ok = true;
            h.exit_ip_vless = Some(ip.clone());
            h.vless_fail = 0;
        } else if xr_configured {
            h.vless_fail = h.vless_fail.saturating_add(1);
            if h.vless_fail >= 2 {
                h.vless_ok = false;
                h.exit_ip_vless = None;
            }
        }
        h.at = Some(chrono::Utc::now());
        (h.wg_ok, h.vless_ok)
    } else {
        (wg_was, xr_was)
    };
    if wg_was != wg_now {
        wlog!(
            "WireGuard канал: {}",
            if !wg_enabled {
                "отключён"
            } else if wg_now {
                "работает"
            } else {
                "не проходит трафик"
            }
        );
    }
    if xr_was != xr_now && (xr_enabled || xr_configured) {
        wlog!(
            "Xray канал: {}",
            if !xr_enabled {
                "отключён"
            } else if xr_now {
                "работает"
            } else {
                "не проходит трафик"
            }
        );
    }
    if xr_configured {
        if !xr_now {
            // автовосстановление: применить рабочий конфиг из пула
            crate::vless::pool::maybe_auto_recover(state, "VLESS мёртв").await;
        } else if let Ok(mut p) = state.pool.lock() {
            if !p.tried.is_empty() {
                p.tried.clear();
                wlog!("VLESS-канал восстановлен");
            }
        }
    }
}

async fn exit_ip(client: &reqwest::Client) -> Option<String> {
    let resp = client.get("https://api.ipify.org").send().await.ok()?;
    let text = resp.text().await.ok()?;
    let ip = text.trim();
    if ip.is_empty() {
        None
    } else {
        Some(ip.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::version_tuple;

    #[test]
    fn compares_versions() {
        assert!(version_tuple("v0.2.1") > version_tuple("0.2.0"));
        assert!(version_tuple("1.0.0") > version_tuple("0.9.9"));
        assert_eq!(version_tuple("v0.2.0"), (0, 2, 0));
        assert_eq!(version_tuple("bad"), (0, 0, 0));
    }
}
