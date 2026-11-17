use serde_json::{json, Value};

use crate::state::AppState;

use super::config::{mask_secret, valid_secret, TelegramConfig, STATUS_FILE};
use super::supervisor::sidecar_path;

fn sidecar_status(st: &AppState) -> Option<Value> {
    let path = crate::paths::in_dir(&st.dir, STATUS_FILE).ok()?;
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<Value>(&text).ok()
}

pub fn status_json(st: &AppState) -> Value {
    let settings = st.settings_snapshot();
    let cfg = TelegramConfig::from_settings(&settings, st.panel_port);
    let sidecar = sidecar_status(st);
    let pid = *st.telegram.pid.lock().unwrap_or_else(|e| e.into_inner());
    let error = st
        .telegram
        .error
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let probe = st
        .telegram
        .probe
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let update = st
        .telegram
        .update
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone();
    let installed_version = super::update::installed_version(&st.dir);
    let mut update = update;
    if update.installed_version.is_none() {
        update.installed_version = installed_version.clone();
    }
    let installed = sidecar_path(&st.dir).is_file();
    let running = pid.is_some();
    let secret_ok = valid_secret(&settings.telegram_secret);
    let preview_order = super::config::order_for(&settings).unwrap_or_default();
    let (order, config_error) = match &cfg {
        Ok(cfg) => (cfg.upstream_order.clone(), None),
        Err(e) => (
            preview_order,
            if settings.telegram_enabled {
                Some(format!("{e:#}"))
            } else {
                None
            },
        ),
    };
    let last_error = if settings.telegram_enabled {
        error.clone().or_else(|| config_error.clone()).or_else(|| {
            sidecar
                .as_ref()
                .and_then(|v| v.get("error"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
    } else {
        None
    };
    let uptime = if running {
        sidecar
            .as_ref()
            .and_then(|v| v.get("uptimeSec"))
            .cloned()
            .or_else(|| {
                st.telegram
                    .last_started
                    .lock()
                    .ok()
                    .and_then(|v| v.map(|t| json!(t.elapsed().as_secs())))
            })
    } else {
        None
    };
    json!({
        "enabled": settings.telegram_enabled,
        "running": running,
        "installed": installed,
        "platform": std::env::consts::OS,
        "sidecarPath": sidecar_path(&st.dir).file_name().and_then(|n| n.to_str()).unwrap_or("tgws-proxy"),
        "pid": pid,
        "host": settings.telegram_host,
        "port": settings.telegram_port,
        "secretMasked": if secret_ok { Some(mask_secret(&settings.telegram_secret)) } else { None },
        "secretConfigured": secret_ok,
        "linkAvailable": running && secret_ok,
        "tryDirect": settings.telegram_try_direct,
        "orderPreset": settings.telegram_order,
        "order": order,
        "cfWorker": settings.telegram_cf_worker,
        "cfProxy": settings.telegram_cf_proxy,
        "fakeTls": settings.telegram_fake_tls,
        "h2": settings.telegram_h2,
        "verbose": settings.telegram_verbose,
        "autoUpdate": settings.telegram_auto_update,
        "installedVersion": installed_version,
        "update": update,
        "error": last_error,
        "version": sidecar.as_ref().and_then(|v| v.get("version")).cloned(),
        "uptimeSec": uptime,
        "restarts": st.telegram.restarts.load(std::sync::atomic::Ordering::Relaxed),
        "statusAt": sidecar.as_ref().and_then(|v| v.get("updatedAt")).cloned(),
        "stats": sidecar.as_ref().and_then(|v| v.get("stats")).cloned(),
        "upstream": sidecar.as_ref().and_then(|v| v.get("upstream")).cloned(),
        "probe": probe,
    })
}
