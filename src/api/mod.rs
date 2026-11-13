//! HTTP-панель и API на 127.0.0.1:8080 + запуск всех сервисов менеджера.
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Request, State};
use axum::http::{Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::engines::EngineKind;
use crate::state::{AppState, CheckEntry};
use crate::wlog;

pub const SMART_PROXY_PORT: u16 = 8081;
pub const PANEL_PORT: u16 = 8080;
const UI_HTML: &str = include_str!("../../ui.html");

pub async fn run(state: Arc<AppState>) -> anyhow::Result<()> {
    let panel_port = state.panel_port;
    wlog!("панель: http://127.0.0.1:{panel_port}/  |  умный прокси: 127.0.0.1:{SMART_PROXY_PORT}");

    // движки: скачиваем отсутствующие, затем запускаем супервизор
    {
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
            st.wg.start();
            st.xr.start();
            if missing && st.interactive {
                let url = format!("http://127.0.0.1:{}/", st.panel_port);
                wlog!("первый запуск: открываю панель {url}");
                if let Err(e) = crate::shell::open_url(&url) {
                    wlog!("панель: не удалось открыть браузер — {e:#}");
                }
            }
        });
    }
    {
        let st = state.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::net::smart_proxy::serve(st).await {
                wlog!("умный прокси остановлен: {e}");
            }
        });
    }
    {
        let st = state.clone();
        tokio::spawn(async move {
            if let Err(e) = crate::net::local_proxy::serve(st).await {
                wlog!("локальные домены остановлены: {e}");
            }
        });
    }
    crate::lists::spawn_scheduler(state.clone());
    crate::vless::pool::spawn_scheduler(state.clone());
    spawn_health_probes(state.clone());

    {
        let st = state.clone();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                wlog!("Ctrl+C: завершение");
                cleanup(&st);
                tokio::time::sleep(Duration::from_millis(700)).await;
                std::process::exit(0);
            }
        });
    }

    let app = Router::new()
        .route("/", get(index))
        .route("/api/status", get(status))
        .route("/proxy.pac", get(pac))
        .route("/api/extra", post(extra))
        .route("/api/vless", post(vless))
        .route("/api/wg", post(wg_config))
        .route("/api/refresh", post(refresh_lists))
        .route("/api/restart-proxy", post(restart_proxy))
        .route("/api/system-proxy", post(system_proxy))
        .route("/api/quit", post(quit))
        .route("/api/auto", post(auto))
        .route(
            "/api/local-domains",
            get(local_domains_get).post(local_domains_post),
        )
        .route("/api/local-domains/probe", post(local_domains_probe))
        .route("/api/local-ports", get(local_ports))
        .route("/api/check", post(check))
        .route("/api/pool/refresh", post(pool_refresh))
        .route("/api/pool/use", post(pool_use))
        .route("/api/subscriptions", post(subscriptions))
        .route("/api/engines/ensure", post(engines_ensure))
        .route("/api/trust-certs", post(trust_certs_handler))
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(state.clone(), guard));

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", panel_port)).await?;
    crate::paths::write_panel_port(&state.dir, panel_port);
    axum::serve(listener, app).await?;
    Ok(())
}

/// Завершение: выключить системный прокси и остановить движки.
pub fn cleanup(state: &AppState) {
    crate::systemproxy::disable_on_exit();
    state.wg.shutdown();
    state.xr.shutdown();
    crate::paths::clear_panel_port(&state.dir);
    wlog!("выход: системный прокси выключен, движки останавливаются");
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

async fn wg_config(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let action = body.get("action").and_then(|v| v.as_str()).unwrap_or("set");
    let path = match crate::paths::in_dir(&st.dir, "wg.conf") {
        Ok(path) => path,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    };
    if action == "clear" {
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(st.dir.join("wireproxy.conf"));
        st.wg.stop();
        wlog!("wg.conf: удалён пользователем");
        return Json(json!({ "ok": true, "configured": false })).into_response();
    }
    let content = body.get("content").and_then(|v| v.as_str()).unwrap_or("");
    if let Err(e) = crate::engines::wireproxy::validate(content) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("{e:#}") })),
        )
            .into_response();
    }
    let tmp = path.with_extension("conf.tmp");
    if let Err(e) = std::fs::write(&tmp, content) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response();
    }
    if path.exists() {
        let _ = std::fs::remove_file(&path);
    }
    if let Err(e) = std::fs::rename(&tmp, &path) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response();
    }
    if let Err(e) = crate::engines::wireproxy::ensure_config(&st.dir) {
        wlog!("wg.conf: wireproxy.conf — {e:#}");
    }
    st.wg.restart();
    let endpoint = content
        .lines()
        .find(|line| line.trim_start().to_lowercase().starts_with("endpoint"))
        .and_then(|line| line.split_once('='))
        .map(|(_, value)| value.trim().to_string())
        .unwrap_or_default();
    wlog!("wg.conf: сохранён (Endpoint {endpoint})");
    Json(json!({ "ok": true, "configured": true, "endpoint": endpoint })).into_response()
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
        st.wg.start();
        st.xr.start();
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
        allowed = domains.iter().any(|d| {
            d.port == st.panel_port
                && (host == d.domain || host == format!("{}:{}", d.domain, st.panel_port))
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

async fn index() -> Html<&'static str> {
    Html(UI_HTML)
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
    let (lists_counts, last_update, update_error, refreshing, vless_only) = {
        let l = st.lists.lock().unwrap();
        (
            (l.proxied.len(), l.direct.len()),
            l.last_update,
            l.error.clone(),
            l.refreshing,
            l.vless_only.clone(),
        )
    };
    let extra = read_domains(&st.dir, "extra-domains.txt");
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
    let checks: Vec<CheckEntry> = st.checks.lock().map(|c| c.clone()).unwrap_or_default();
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
                    "ms": e.ms
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
    let visited: Vec<Value> = {
        let v = st.visited.lock().unwrap();
        let mut items = v.items.clone();
        items.sort_by_key(|(_, _, ts)| std::cmp::Reverse(*ts));
        items
            .into_iter()
            .take(60)
            .map(|(domain, count, last)| json!({ "domain": domain, "count": count, "last": last }))
            .collect()
    };
    let engines_ready = crate::engines::find_engine(EngineKind::Wireproxy).is_some()
        && crate::engines::find_engine(EngineKind::Xray).is_some();
    let downloads = st.downloads.lock().map(|d| d.clone()).unwrap_or_default();

    Json(json!({
        "vpnUp": st.wg.is_running(),
        "wgConfigured": crate::engines::wireproxy::is_configured(&st.dir),
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
            "domains": local_domains.iter().map(|d| json!({ "domain": d.domain, "port": d.port })).collect::<Vec<_>>(),
        },
        "vpnDefault": settings.vpn_default,
        "autostart": crate::autostart::installed(),
        "caTrusted": crate::trust_status::is_trusted(&st.dir),
        "enginesReady": engines_ready,
        "engineDownloads": downloads,
        "vlessDomains": vless_only.into_iter().collect::<Vec<_>>(),
        "proxied": lists_counts.0,
        "direct": lists_counts.1,
        "lastUpdate": last_update,
        "updateError": update_error,
        "refreshing": refreshing,
        "mode": crate::lists::MODE,
        "systemProxy": sys_proxy,
        "extra": extra,
        "checks": checks,
        "visited": visited,
    }))
}

async fn extra(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let action = body.get("action").and_then(|v| v.as_str()).unwrap_or("");
    let Some(domain) = body
        .get("domain")
        .and_then(|v| v.as_str())
        .and_then(crate::lists::normalize_domain)
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "некорректный домен" })),
        )
            .into_response();
    };
    let mut list = read_domains(&st.dir, "extra-domains.txt");
    match action {
        "add" => {
            if !list.contains(&domain) {
                list.push(domain.clone());
            }
        }
        "remove" => list.retain(|d| *d != domain),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "action должен быть add|remove" })),
            )
                .into_response();
        }
    }
    if let Err(e) = write_domains(&st.dir, "extra-domains.txt", &list) {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response();
    }
    spawn_refresh(&st);
    Json(json!({ "ok": true, "extra": read_domains(&st.dir, "extra-domains.txt") })).into_response()
}

async fn vless(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let action = body.get("action").and_then(|v| v.as_str()).unwrap_or("");
    match action {
        "set-link" => {
            let link = body.get("link").and_then(|v| v.as_str()).unwrap_or("");
            match crate::vless::link::save_link(&st.dir, link) {
                Ok(info) => {
                    wlog!("VLESS-конфиг сохранён: {info:?}");
                    st.xr.restart();
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
            let _ = std::fs::remove_file(st.dir.join("xray-config.json"));
            st.xr.stop();
            Json(json!({ "ok": true })).into_response()
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
            let mut s = st.settings.lock().unwrap();
            s.vpn_default = value.to_string();
            let _ = s.save(&st.dir);
            Json(json!({ "ok": true, "vpnDefault": s.vpn_default })).into_response()
        }
        "add-domain" | "remove-domain" => {
            let Some(domain) = body
                .get("domain")
                .and_then(|v| v.as_str())
                .and_then(crate::lists::normalize_domain)
            else {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({ "error": "некорректный домен" })),
                )
                    .into_response();
            };
            let mut list = read_domains(&st.dir, "vless-domains.txt");
            if action == "add-domain" {
                if !list.contains(&domain) {
                    list.push(domain);
                }
            } else {
                list.retain(|d| *d != domain);
            }
            list.sort();
            list.dedup();
            if let Err(e) = write_domains(&st.dir, "vless-domains.txt", &list) {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                )
                    .into_response();
            }
            if let Ok(mut l) = st.lists.lock() {
                l.vless_only = list.iter().cloned().collect();
            }
            Json(json!({ "ok": true, "vlessDomains": list })).into_response()
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

async fn restart_proxy(State(st): State<Arc<AppState>>) -> Json<Value> {
    wlog!("перезапуск wireproxy по запросу из панели");
    st.wg.restart();
    Json(json!({ "ok": true }))
}

async fn system_proxy(State(_st): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let enabled = body
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    match crate::systemproxy::set_system_proxy(enabled) {
        Ok(state) => Json(json!({ "ok": true, "state": state })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn auto(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let mut s = st.settings.lock().unwrap();
    if let Some(v) = body.get("autoRotate").and_then(|v| v.as_bool()) {
        s.auto_rotate = v;
    }
    if let Some(v) = body.get("autoPoolRefreshHours").and_then(|v| v.as_f64()) {
        if !(0.0..=168.0).contains(&v) {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "autoPoolRefreshHours: 0..168" })),
            )
                .into_response();
        }
        s.auto_pool_refresh_hours = v;
    }
    let _ = s.save(&st.dir);
    Json(json!({ "ok": true, "autoRotate": s.auto_rotate, "autoPoolRefreshHours": s.auto_pool_refresh_hours })).into_response()
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
    let domain = body
        .get("domain")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if domain.is_empty()
        || !domain
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "некорректный домен" })),
        )
            .into_response();
    }
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
    if let Ok(output) = Command::new("netstat").args(["-ano", "-p", "TCP"]).output() {
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
        .and_then(crate::lists::normalize_domain)
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "некорректный домен" })),
        )
            .into_response();
    };
    let url = format!("https://{domain}/");
    let direct_client = st.client("check-direct", None);
    let direct = http_code(&direct_client, &url).await;

    let settings = st.settings_snapshot();
    let (_, vless_ok) = st.health_snapshot();
    let port = if settings.vpn_default == "vless" && vless_ok {
        crate::vless::XRAY_HTTP_PORT
    } else {
        crate::vless::WIREPROXY_HTTP_PORT
    };
    let vpn_client = st.client(&format!("check-vpn-{port}"), Some(port));
    let vpn = http_code(&vpn_client, &url).await;

    let ok = |c: &str| c != "000" && c != "403" && c != "451";
    let verdict = if ok(&vpn) && !ok(&direct) {
        "vpn-helps"
    } else if ok(&direct) && !ok(&vpn) {
        "direct-only"
    } else if ok(&direct) && ok(&vpn) {
        "both-ok"
    } else if vpn == "000" {
        "blocked-everywhere"
    } else {
        "challenge"
    };
    let entry = CheckEntry {
        domain: domain.clone(),
        direct: direct.clone(),
        vpn: vpn.clone(),
        verdict: verdict.to_string(),
        at: chrono::Utc::now().to_rfc3339(),
    };
    if let Ok(mut checks) = st.checks.lock() {
        checks.insert(0, entry.clone());
        checks.truncate(25);
    }
    wlog!("проверка {domain}: напрямую {direct}, через VPN {vpn} -> {verdict}");
    Json(json!({ "domain": entry.domain, "direct": entry.direct, "vpn": entry.vpn, "verdict": entry.verdict, "at": entry.at })).into_response()
}

async fn http_code(client: &reqwest::Client, url: &str) -> String {
    match client.get(url).send().await {
        Ok(resp) => resp.status().as_u16().to_string(),
        Err(_) => "000".to_string(),
    }
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

async fn pool_use(State(st): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let index = body
        .get("index")
        .and_then(|v| v.as_u64())
        .unwrap_or(u64::MAX) as usize;
    match crate::vless::pool::apply_entry(&st, index) {
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
        "remove" => list.retain(|u| *u != url),
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "action должен быть add|remove" })),
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
    Json(json!({ "ok": true, "subscriptions": crate::vless::pool::read_subscriptions(&st.dir) }))
        .into_response()
}

// ---------------- файловые помощники ----------------

pub fn read_domains(dir: &std::path::Path, name: &str) -> Vec<String> {
    crate::lists::read_domain_file(dir, name)
}

pub fn write_domains(dir: &std::path::Path, name: &str, list: &[String]) -> std::io::Result<()> {
    let path = crate::paths::in_dir(dir, name)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()))?;
    let mut text = String::from("# wireproxy-control: домены (по одному в строке)\n\n");
    for d in list {
        text.push_str(d);
        text.push('\n');
    }
    std::fs::write(path, text)
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
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });
}

async fn probe_once(state: &Arc<AppState>) {
    let wg_client = state.client("probe-wg", Some(crate::vless::WIREPROXY_HTTP_PORT));
    let wg_ip = exit_ip(&wg_client).await;
    let xr_client = state.client("probe-xr", Some(crate::vless::XRAY_HTTP_PORT));
    let xr_ip = if crate::engines::xray::has_config(&state.dir) {
        exit_ip(&xr_client).await
    } else {
        None
    };
    let wg_was = state.health.lock().map(|h| h.wg_ok).unwrap_or(false);
    let xr_was = state.health.lock().map(|h| h.vless_ok).unwrap_or(false);
    if let Ok(mut h) = state.health.lock() {
        h.wg_ok = wg_ip.is_some();
        h.exit_ip = wg_ip.clone();
        h.vless_ok = xr_ip.is_some();
        h.exit_ip_vless = xr_ip.clone();
        h.at = Some(chrono::Utc::now());
    }
    if wg_was != wg_ip.is_some() {
        wlog!(
            "WireGuard канал: {}",
            if wg_ip.is_some() {
                "работает"
            } else {
                "не проходит трафик"
            }
        );
    }
    if xr_was != xr_ip.is_some() && crate::engines::xray::has_config(&state.dir) {
        wlog!(
            "VLESS канал: {}",
            if xr_ip.is_some() {
                "работает"
            } else {
                "не проходит трафик"
            }
        );
    }
    if crate::engines::xray::has_config(&state.dir) {
        if xr_ip.is_none() {
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
