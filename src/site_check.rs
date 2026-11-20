//! Проверка доменов (напрямую / через VPN / через Zapret) и фоновая
//! проверка новых доменов из «Недавних сайтов».
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use crate::state::{AppState, CheckEntry};
use crate::wlog;

async fn http_code(client: &reqwest::Client, url: &str) -> String {
    match client.get(url).send().await {
        Ok(resp) => resp.status().as_u16().to_string(),
        Err(_) => "000".to_string(),
    }
}

pub async fn check_domain(st: &Arc<AppState>, domain: &str) -> CheckEntry {
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

    let zapret_supported = crate::zapret::supported();
    let zapret_running = zapret_supported && crate::zapret::service_running_cached(st);
    let zapret = if zapret_running {
        let client = st.client("check-zapret", None);
        http_code(&client, &url).await
    } else {
        "n/a".to_string()
    };

    let ok = |c: &str| c != "000" && c != "403" && c != "451";
    let verdict = if ok(&vpn) && !ok(&direct) {
        "vpn-helps"
    } else if ok(&direct) && !ok(&vpn) {
        "direct-only"
    } else if ok(&direct) && ok(&vpn) {
        "both-ok"
    } else if zapret_running && ok(&zapret) && !ok(&vpn) {
        "zapret-helps"
    } else if vpn == "000" {
        "blocked-everywhere"
    } else {
        "challenge"
    };
    let entry = CheckEntry {
        domain: domain.to_string(),
        direct: direct.clone(),
        vpn: vpn.clone(),
        zapret: zapret.clone(),
        zapret_running,
        zapret_supported,
        verdict: verdict.to_string(),
        at: chrono::Utc::now().to_rfc3339(),
    };
    if let Err(e) = st.store.insert_check(&entry) {
        wlog!("история проверок: {e:#}");
    }
    wlog!("проверка {domain}: напрямую {direct}, через VPN {vpn}, Zapret {zapret} -> {verdict}");
    entry
}

fn valid_candidate(domain: &str) -> bool {
    if domain.ends_with(".localhost") || domain.ends_with(".local") {
        return false;
    }
    crate::lists::validate_hostname(domain).is_some()
}

async fn add_unavailable(st: &Arc<AppState>, domain: &str) {
    use std::sync::OnceLock;
    static ADD_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    let _guard = ADD_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    let mut extra = crate::api::read_domains(&st.dir, "extra-domains.txt");
    if extra.iter().any(|d| d == domain) {
        return;
    }
    let blocked: HashSet<String> = ["vless-domains.txt", "wg-domains.txt"]
        .into_iter()
        .flat_map(|name| crate::api::read_domains(&st.dir, name))
        .collect();
    if blocked.contains(domain) {
        return;
    }
    extra.push(domain.to_string());
    if let Err(e) = crate::api::write_domains(&st.dir, "extra-domains.txt", &extra) {
        wlog!("автопроверка: не удалось добавить {domain} — {e}");
        return;
    }
    wlog!("автопроверка: {domain} недоступен напрямую, добавлен в VPN-список");
    let st = st.clone();
    tokio::spawn(async move {
        if let Err(e) = crate::lists::refresh(&st).await {
            wlog!("автопроверка: обновление списков — {e}");
        }
    });
}

async fn run_cycle(st: &Arc<AppState>) {
    let settings = st.settings_snapshot();
    if !settings.auto_check_sites {
        return;
    }
    let now = chrono::Utc::now().timestamp();
    let cutoff = now - 86_400;
    let checked = st.store.checked_domains(cutoff).unwrap_or_default();
    let candidates: Vec<String> = st
        .store
        .visited(cutoff, 50)
        .unwrap_or_default()
        .into_iter()
        .filter(|row| !checked.contains(&row.domain) && valid_candidate(&row.domain))
        .map(|row| row.domain)
        .take(3)
        .collect();
    for domain in candidates {
        if let Err(e) = st.store.mark_checked(&domain, now) {
            wlog!("автопроверка: {e:#}");
        }
        let entry = check_domain(st, &domain).await;
        if settings.auto_add_unavailable && entry.verdict == "vpn-helps" {
            add_unavailable(st, &domain).await;
        }
    }
}

pub fn spawn_scheduler(st: Arc<AppState>) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(20)).await;
        loop {
            run_cycle(&st).await;
            tokio::time::sleep(Duration::from_secs(90)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::valid_candidate;

    #[test]
    fn skips_local_and_invalid_hosts() {
        assert!(valid_candidate("example.com"));
        assert!(!valid_candidate("app.localhost"));
        assert!(!valid_candidate("printer.local"));
        assert!(!valid_candidate("127.0.0.1"));
        assert!(!valid_candidate("bad host"));
    }
}
