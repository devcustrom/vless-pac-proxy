//! Проверка доменов (напрямую / WireGuard / VLESS / Zapret) и фоновая
//! проверка новых доменов из «Недавних сайтов».
use std::sync::Arc;
use std::time::Duration;

use crate::route_auto::{ProbeResult, ProbeSet, RouteDecision};
use crate::routing::AutoChannel;
use crate::state::{AppState, CheckEntry};
use crate::wlog;

pub struct CheckOutcome {
    pub entry: CheckEntry,
    pub probes: ProbeSet,
    pub best: Option<RouteDecision>,
    pub in_auto: bool,
}

fn best_vpn(probes: &ProbeSet) -> Option<(AutoChannel, &ProbeResult)> {
    let mut items: Vec<(AutoChannel, &ProbeResult)> = Vec::new();
    if let Some(item) = probes.wireguard.as_ref().filter(|item| item.usable()) {
        items.push((AutoChannel::Wireguard, item));
    }
    if let Some(item) = probes.vless.as_ref().filter(|item| item.usable()) {
        items.push((AutoChannel::Vless, item));
    }
    items.into_iter().min_by_key(|(_, item)| item.ms)
}

pub async fn check_domain_full(st: &Arc<AppState>, domain: &str) -> CheckOutcome {
    let probes = match crate::route_auto::probe(st, domain).await {
        Ok(probes) => probes,
        Err(e) => {
            wlog!("проверка {domain}: {e:#}");
            ProbeSet {
                direct: ProbeResult {
                    code: "000".to_string(),
                    ms: 0,
                },
                ..ProbeSet::default()
            }
        }
    };

    let direct = probes.direct.code.clone();
    let vpn_best = best_vpn(&probes);
    let vpn = vpn_best
        .map(|(_, item)| item.code.clone())
        .or_else(|| probes.wireguard.as_ref().map(|item| item.code.clone()))
        .or_else(|| probes.vless.as_ref().map(|item| item.code.clone()))
        .unwrap_or_else(|| "000".to_string());
    let zapret = if probes.zapret_running {
        direct.clone()
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
    } else if probes.zapret_running && ok(&zapret) && !ok(&vpn) {
        "zapret-helps"
    } else if vpn == "000" {
        "blocked-everywhere"
    } else {
        "challenge"
    };

    let in_auto = crate::lists::read_domain_file(&st.dir, "extra-domains.txt")
        .iter()
        .any(|item| item == domain);
    let best = if in_auto {
        crate::route_auto::store_decision(st, domain, &probes)
    } else {
        crate::route_auto::choose(None, &probes)
    };

    let entry = CheckEntry {
        domain: domain.to_string(),
        direct: direct.clone(),
        vpn: vpn.clone(),
        zapret: zapret.clone(),
        zapret_running: probes.zapret_running,
        zapret_supported: crate::zapret::supported(),
        verdict: verdict.to_string(),
        at: chrono::Utc::now().to_rfc3339(),
    };
    if let Err(e) = st.store.insert_check(&entry) {
        wlog!("история проверок: {e:#}");
    }
    wlog!("проверка {domain}: напрямую {direct}, VPN {vpn}, Zapret {zapret} -> {verdict}");
    CheckOutcome {
        entry,
        probes,
        best,
        in_auto,
    }
}

pub async fn check_domain(st: &Arc<AppState>, domain: &str) -> CheckEntry {
    check_domain_full(st, domain).await.entry
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
    // Явное исключение «Напрямую» не трогаем: пользователь выбрал его сам.
    if crate::route_auto::is_direct_domain(st, domain) {
        return;
    }
    let mut extra = crate::api::read_domains(&st.dir, "extra-domains.txt");
    if extra.iter().any(|d| d == domain) {
        return;
    }
    extra.push(domain.to_string());
    if let Err(e) = crate::api::write_domains(&st.dir, "extra-domains.txt", &extra) {
        wlog!("автопроверка: не удалось добавить {domain} — {e}");
        return;
    }
    crate::zapret::sync_domains(st, &extra);
    crate::route_auto::spawn_check_one(st.clone(), domain.to_string());
    wlog!("автопроверка: {domain} недоступен напрямую, добавлен в авто-список");
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
