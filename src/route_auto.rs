//! Автоматический выбор маршрута для пользовательского списка доменов.
//!
//! Раз в 10 минут (и сразу после добавления) домен проверяется напрямую,
//! через WireGuard и через VLESS. Выбирается самый быстрый рабочий путь.
//! Решения хранятся в SQLite (`wireproxy.db`, таблица `auto_routes`)
//! и переживают перезапуск. Старый `auto-routes.json` импортируется один раз.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::routing::AutoChannel;
use crate::state::AppState;
use crate::wlog;

const RECHECK_SECS: i64 = 600;
const BATCH: usize = 12;
const PROBE_TIMEOUT: Duration = Duration::from_secs(6);
/// Новый путь должен быть быстрее текущего минимум на 20% и на 30 мс.
const HYSTERESIS_PERCENT: u64 = 80;
const HYSTERESIS_MS: u64 = 30;
const ROUTES_FILE: &str = "auto-routes.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProbeResult {
    pub code: String,
    pub ms: u64,
}

impl ProbeResult {
    fn new(code: impl Into<String>, ms: u64) -> Self {
        Self {
            code: code.into(),
            ms,
        }
    }

    /// 000/403/451 и 5xx не считаем рабочим маршрутом.
    pub fn usable(&self) -> bool {
        if self.code == "000" {
            return false;
        }
        match self.code.parse::<u16>() {
            Ok(code) => code < 500 && code != 403 && code != 451,
            Err(_) => false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ProbeSet {
    pub direct: ProbeResult,
    pub wireguard: Option<ProbeResult>,
    pub vless: Option<ProbeResult>,
    pub zapret_running: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteDecision {
    #[serde(rename = "route")]
    pub channel: AutoChannel,
    pub available: bool,
    pub latency_ms: u64,
    pub direct: ProbeResult,
    pub wireguard: Option<ProbeResult>,
    pub vless: Option<ProbeResult>,
    pub via_zapret: bool,
    pub checked_at: i64,
}

impl RouteDecision {
    /// Более быстрый из двух VPN-путей, даже если прямой оказался быстрее.
    pub fn fastest_vpn(&self) -> Option<AutoChannel> {
        let mut items = Vec::new();
        if let Some(probe) = self.wireguard.as_ref().filter(|probe| probe.code != "000") {
            items.push((AutoChannel::Wireguard, probe.ms));
        }
        if let Some(probe) = self.vless.as_ref().filter(|probe| probe.code != "000") {
            items.push((AutoChannel::Vless, probe.ms));
        }
        items
            .into_iter()
            .min_by_key(|(_, ms)| *ms)
            .map(|(channel, _)| channel)
    }
}

#[derive(Debug, Clone, Default)]
pub struct AutoRoutes {
    pub routes: HashMap<String, RouteDecision>,
    pub checking: HashSet<String>,
}

impl AutoRoutes {
    pub fn from_store(store: &crate::store::Store) -> Self {
        Self {
            routes: store.auto_routes().unwrap_or_default(),
            checking: HashSet::new(),
        }
    }

    pub fn decision(&self, domain: &str) -> Option<RouteDecision> {
        self.routes.get(domain).cloned()
    }
}

#[derive(Deserialize)]
struct LegacyRoutes {
    #[serde(default)]
    routes: HashMap<String, RouteDecision>,
}

/// Импортирует старый auto-routes.json в SQLite и убирает файл.
pub fn import_legacy_json(dir: &std::path::Path, store: &crate::store::Store) {
    let Ok(path) = crate::paths::in_dir(dir, ROUTES_FILE) else {
        return;
    };
    if !path.is_file() {
        return;
    }
    let already = store
        .auto_routes()
        .map(|routes| !routes.is_empty())
        .unwrap_or(false);
    if !already {
        match std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<LegacyRoutes>(&text).ok())
        {
            Some(legacy) => {
                for (domain, decision) in legacy.routes {
                    if let Err(e) = store.save_auto_route(&domain, &decision) {
                        wlog!("авто-маршрут: импорт {domain} в БД — {e:#}");
                    }
                }
            }
            None => wlog!("авто-маршрут: не удалось разобрать старый {ROUTES_FILE}"),
        }
    }
    let target = path.with_extension("json.migrated");
    let _ = std::fs::remove_file(&target);
    if let Err(e) = std::fs::rename(&path, &target) {
        wlog!("авто-маршрут: не удалось убрать старый {ROUTES_FILE} — {e:#}");
    }
}

/// Возвращает выбранный маршрут для домена из сохранённых решений.
pub fn decision_for(st: &AppState, domain: &str) -> Option<RouteDecision> {
    st.auto_routes
        .lock()
        .ok()
        .and_then(|routes| routes.decision(&domain.to_lowercase()))
}

pub fn remove_decision(st: &AppState, domain: &str) {
    let domain = domain.to_lowercase();
    st.auto_routes
        .lock()
        .map(|mut routes| routes.routes.remove(&domain))
        .ok();
    if let Err(e) = st.store.delete_auto_route(&domain) {
        wlog!("авто-маршрут: удаление {domain} из БД — {e:#}");
    }
}

/// Убирает решения для доменов, которых больше нет в авто-списке.
pub fn prune_decisions(st: &AppState) {
    let keep: HashSet<String> = auto_domains(st).into_iter().collect();
    if let Ok(mut routes) = st.auto_routes.lock() {
        routes.routes.retain(|domain, _| keep.contains(domain));
    }
    if let Err(e) = st.store.prune_auto_routes(&keep) {
        wlog!("авто-маршрут: очистка БД — {e:#}");
    }
}

pub fn auto_domains(st: &AppState) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for domain in crate::lists::read_domain_file(&st.dir, "extra-domains.txt") {
        if seen.insert(domain.clone()) {
            items.push(domain);
        }
    }
    items
}

fn direct_domains(st: &AppState) -> Vec<String> {
    crate::lists::read_domain_file(&st.dir, "direct-domains.txt")
}

/// Явное исключение «Напрямую» действует и на поддомены.
pub fn is_direct_domain(st: &AppState, domain: &str) -> bool {
    matches_suffix(&direct_domains(st), domain)
}

fn matches_suffix(items: &[String], domain: &str) -> bool {
    let mut current = domain.to_lowercase();
    loop {
        if items.iter().any(|item| item == &current) {
            return true;
        }
        match current.find('.') {
            Some(i) => current = current[i + 1..].to_string(),
            None => return false,
        }
    }
}

/// Проверяет домен всеми доступными путями.
pub async fn probe(st: &Arc<AppState>, domain: &str) -> Result<ProbeSet> {
    let domain = crate::lists::validate_hostname(domain).context("некорректный домен")?;
    let url = format!("https://{domain}/");
    let settings = st.settings_snapshot();
    let zapret_running = crate::zapret::supported() && crate::zapret::service_running_cached(st);
    if let Err(e) = crate::net::dns_guard::check_host(&domain).await {
        wlog!("авто-маршрут {domain}: проверка пропущена — {e:#}");
        return Ok(ProbeSet {
            direct: ProbeResult::new("000", 0),
            ..ProbeSet::default()
        });
    }

    let direct_fut = async { measure(None, &url).await };
    let wg_fut = async {
        if settings.wireguard_enabled && st.wg.is_running() {
            Some(measure(Some(crate::vless::WIREPROXY_HTTP_PORT), &url).await)
        } else {
            None
        }
    };
    let vless_fut = async {
        if settings.xray_enabled && st.xr.is_running() {
            Some(measure(Some(crate::vless::XRAY_HTTP_PORT), &url).await)
        } else {
            None
        }
    };

    let (direct, wireguard, vless) = tokio::join!(direct_fut, wg_fut, vless_fut);
    Ok(ProbeSet {
        direct,
        wireguard,
        vless,
        zapret_running,
    })
}

/// Один замер без переиспользования idle-соединений.
async fn measure(proxy_port: Option<u16>, url: &str) -> ProbeResult {
    let mut builder = reqwest::Client::builder()
        .timeout(PROBE_TIMEOUT)
        .user_agent("wireproxy-control/route-probe")
        .pool_max_idle_per_host(0)
        .redirect(reqwest::redirect::Policy::limited(5));
    if let Some(port) = proxy_port {
        if let Ok(proxy) = reqwest::Proxy::all(format!("http://127.0.0.1:{port}")) {
            builder = builder.proxy(proxy);
        }
    } else {
        builder = builder.no_proxy();
    }
    let client = match builder.build() {
        Ok(client) => client,
        Err(_) => return ProbeResult::new("000", 0),
    };
    let started = Instant::now();
    match client.get(url).send().await {
        Ok(response) => ProbeResult::new(
            response.status().as_u16().to_string(),
            started.elapsed().as_millis() as u64,
        ),
        Err(_) => ProbeResult::new("000", started.elapsed().as_millis() as u64),
    }
}

fn probe_for(probes: &ProbeSet, channel: AutoChannel) -> Option<&ProbeResult> {
    match channel {
        AutoChannel::Direct => Some(&probes.direct),
        AutoChannel::Wireguard => probes.wireguard.as_ref(),
        AutoChannel::Vless => probes.vless.as_ref(),
    }
}

fn build_decision(
    channel: AutoChannel,
    probes: &ProbeSet,
    checked_at: i64,
    available: bool,
) -> RouteDecision {
    RouteDecision {
        channel,
        available,
        latency_ms: probe_for(probes, channel).map(|p| p.ms).unwrap_or(0),
        direct: probes.direct.clone(),
        wireguard: probes.wireguard.clone(),
        vless: probes.vless.clone(),
        via_zapret: channel == AutoChannel::Direct && probes.zapret_running,
        checked_at,
    }
}

/// Выбирает самый быстрый рабочий путь, не дёргая маршрут из-за шума.
pub fn choose(previous: Option<&RouteDecision>, probes: &ProbeSet) -> Option<RouteDecision> {
    let checked_at = chrono::Utc::now().timestamp();
    let mut candidates: Vec<(AutoChannel, u64)> = Vec::new();
    if probes.direct.usable() {
        candidates.push((AutoChannel::Direct, probes.direct.ms));
    }
    if let Some(item) = probes.wireguard.as_ref().filter(|item| item.usable()) {
        candidates.push((AutoChannel::Wireguard, item.ms));
    }
    if let Some(item) = probes.vless.as_ref().filter(|item| item.usable()) {
        candidates.push((AutoChannel::Vless, item.ms));
    }

    let Some((fastest, fastest_ms)) = candidates.iter().min_by_key(|(_, ms)| *ms).copied() else {
        return previous.map(|prev| RouteDecision {
            available: false,
            direct: probes.direct.clone(),
            wireguard: probes.wireguard.clone(),
            vless: probes.vless.clone(),
            checked_at,
            ..prev.clone()
        });
    };

    if let Some(prev) = previous {
        if let Some(item) = probe_for(probes, prev.channel) {
            if item.usable() {
                let enough_faster = fastest_ms.saturating_mul(100)
                    < item.ms.saturating_mul(HYSTERESIS_PERCENT)
                    && item.ms > fastest_ms.saturating_add(HYSTERESIS_MS);
                if !enough_faster {
                    return Some(build_decision(prev.channel, probes, checked_at, true));
                }
            }
        }
    }

    Some(build_decision(fastest, probes, checked_at, true))
}

/// Сохраняет решение с учётом предыдущего выбора.
pub fn store_decision(
    st: &Arc<AppState>,
    domain: &str,
    probes: &ProbeSet,
) -> Option<RouteDecision> {
    let domain = crate::lists::validate_hostname(domain)?;
    if !auto_domains(st).iter().any(|item| item == &domain) {
        return None;
    }
    let previous = decision_for(st, &domain);
    let decision = choose(previous.as_ref(), probes)?;
    if let Ok(mut routes) = st.auto_routes.lock() {
        routes.routes.insert(domain.clone(), decision.clone());
    }
    let dcode = &decision.direct.code;
    let dms = decision.direct.ms;
    let wcode = decision
        .wireguard
        .as_ref()
        .map(|p| p.code.as_str())
        .unwrap_or("n/a");
    let wms = decision.wireguard.as_ref().map(|p| p.ms).unwrap_or(0);
    let vcode = decision
        .vless
        .as_ref()
        .map(|p| p.code.as_str())
        .unwrap_or("n/a");
    let vms = decision.vless.as_ref().map(|p| p.ms).unwrap_or(0);
    let channel = decision.channel;
    let suffix = if decision.via_zapret { " (Zapret)" } else { "" };
    wlog!(
        "авто-маршрут {domain}: напрямую {dcode}/{dms}мс, WireGuard {wcode}/{wms}мс, \
         VLESS {vcode}/{vms}мс -> {channel:?}{suffix}"
    );
    if let Err(e) = st.store.save_auto_route(&domain, &decision) {
        wlog!("авто-маршрут: сохранение {domain} в БД — {e:#}");
    }
    Some(decision)
}

struct CheckingGuard {
    state: Arc<AppState>,
    domain: String,
}

impl Drop for CheckingGuard {
    fn drop(&mut self) {
        if let Ok(mut routes) = self.state.auto_routes.lock() {
            routes.checking.remove(&self.domain);
        }
    }
}

/// Проверяет домен и обновляет сохранённое решение.
pub async fn check_and_store(st: &Arc<AppState>, domain: &str) -> Option<RouteDecision> {
    let domain = crate::lists::validate_hostname(domain)?;
    {
        let mut routes = st.auto_routes.lock().ok()?;
        if !routes.checking.insert(domain.clone()) {
            return None;
        }
    }
    let _guard = CheckingGuard {
        state: st.clone(),
        domain: domain.clone(),
    };
    match probe(st, &domain).await {
        Ok(probes) => store_decision(st, &domain, &probes),
        Err(e) => {
            wlog!("авто-маршрут {domain}: {e:#}");
            None
        }
    }
}

fn network_enabled() -> bool {
    std::env::var_os("WPC_TEST_MODE").is_none()
}

pub fn spawn_check_one(st: Arc<AppState>, domain: String) {
    if !network_enabled() {
        return;
    }
    tokio::spawn(async move {
        check_and_store(&st, &domain).await;
    });
}

pub fn spawn_check_all(st: Arc<AppState>) {
    if !network_enabled() {
        return;
    }
    tokio::spawn(async move {
        check_all(&st).await;
    });
}

async fn check_all(st: &Arc<AppState>) {
    let direct = direct_domains(st);
    for domain in auto_domains(st) {
        if matches_suffix(&direct, &domain) {
            continue;
        }
        check_and_store(st, &domain).await;
    }
}

async fn run_cycle(st: &Arc<AppState>) {
    let now = chrono::Utc::now().timestamp();
    let direct = direct_domains(st);
    let mut pending: Vec<(String, i64)> = auto_domains(st)
        .into_iter()
        .filter(|domain| !matches_suffix(&direct, domain))
        .map(|domain| {
            let checked = decision_for(st, &domain)
                .map(|decision| decision.checked_at)
                .unwrap_or(0);
            (domain, checked)
        })
        .filter(|(_, checked)| now.saturating_sub(*checked) >= RECHECK_SECS)
        .collect();
    pending.sort_by_key(|(_, checked)| *checked);
    pending.truncate(BATCH);
    for (domain, _) in pending {
        check_and_store(st, &domain).await;
    }
}

pub fn spawn_scheduler(st: Arc<AppState>) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(10)).await;
        loop {
            run_cycle(&st).await;
            tokio::time::sleep(Duration::from_secs(RECHECK_SECS as u64)).await;
        }
    });
}

fn retire_legacy_lists(dir: &std::path::Path) {
    for name in ["vless-domains.txt", "wg-domains.txt"] {
        let Ok(path) = crate::paths::in_dir(dir, name) else {
            continue;
        };
        if !path.exists() {
            continue;
        }
        let target = path.with_extension("txt.migrated");
        let _ = std::fs::remove_file(&target);
        if let Err(e) = std::fs::rename(&path, &target) {
            wlog!("миграция списков: не удалось переименовать {name} — {e:#}");
        }
    }
}

fn merge_forced_lists(dir: &std::path::Path) -> Result<Option<Vec<String>>> {
    let vless = crate::lists::read_domain_file(dir, "vless-domains.txt");
    let wg = crate::lists::read_domain_file(dir, "wg-domains.txt");
    if vless.is_empty() && wg.is_empty() {
        retire_legacy_lists(dir);
        return Ok(None);
    }
    let mut extra = Vec::new();
    let mut seen = HashSet::new();
    for domain in crate::lists::read_domain_file(dir, "extra-domains.txt") {
        if seen.insert(domain.clone()) {
            extra.push(domain);
        }
    }
    let direct = crate::lists::read_domain_file(dir, "direct-domains.txt");
    for domain in vless.into_iter().chain(wg) {
        if matches_suffix(&direct, &domain) || !seen.insert(domain.clone()) {
            continue;
        }
        extra.push(domain);
    }
    crate::lists::write_domain_file(dir, "extra-domains.txt", &extra)?;
    retire_legacy_lists(dir);
    Ok(Some(extra))
}

/// Сливает старые списки «только WireGuard/VLESS» в общий авто-список.
pub fn migrate_lists(st: &Arc<AppState>) {
    let extra = match merge_forced_lists(&st.dir) {
        Ok(Some(extra)) => extra,
        Ok(None) => return,
        Err(e) => {
            wlog!("миграция списков: {e:#}");
            return;
        }
    };
    prune_decisions(st);
    crate::zapret::sync_domains(st, &extra);
    wlog!("миграция списков: старые списки объединены в авто-список");
}

/// Убирает из авто-списка домены, покрытые исключениями, и чинит закрепления.
pub fn reconcile_lists(st: &Arc<AppState>) {
    let direct = direct_domains(st);
    let mut extra = auto_domains(st);
    let mut pins = crate::lists::read_domain_file(&st.dir, "vpn-only-domains.txt");
    let before_extra = extra.len();
    let before_pins = pins.len();
    extra.retain(|domain| !matches_suffix(&direct, domain));
    pins.retain(|domain| !matches_suffix(&direct, domain));
    for domain in &pins {
        if !extra.contains(domain) {
            extra.push(domain.clone());
        }
    }
    if extra.len() == before_extra && pins.len() == before_pins {
        return;
    }
    if let Err(e) = crate::lists::write_domain_file(&st.dir, "extra-domains.txt", &extra) {
        wlog!("маршруты: не удалось очистить авто-список от исключений — {e:#}");
        return;
    }
    if let Err(e) = crate::lists::write_domain_file(&st.dir, "vpn-only-domains.txt", &pins) {
        wlog!("маршруты: не удалось обновить закрепления VPN — {e:#}");
    }
    prune_decisions(st);
    crate::zapret::sync_domains(st, &extra);
    wlog!("маршруты: списки и закрепления приведены в порядок");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(code: &str, ms: u64) -> ProbeResult {
        ProbeResult::new(code, ms)
    }

    fn set(direct: ProbeResult, wg: Option<ProbeResult>, vless: Option<ProbeResult>) -> ProbeSet {
        ProbeSet {
            direct,
            wireguard: wg,
            vless,
            zapret_running: false,
        }
    }

    #[test]
    fn chooses_fastest_working_path() {
        let probes = set(
            probe("200", 120),
            Some(probe("200", 80)),
            Some(probe("200", 200)),
        );
        let choice = choose(None, &probes).unwrap();
        assert_eq!(choice.channel, AutoChannel::Wireguard);
        assert_eq!(choice.latency_ms, 80);
    }

    #[test]
    fn ignores_blocked_and_server_errors() {
        let probes = set(
            probe("403", 30),
            Some(probe("500", 40)),
            Some(probe("200", 90)),
        );
        let choice = choose(None, &probes).unwrap();
        assert_eq!(choice.channel, AutoChannel::Vless);
    }

    #[test]
    fn keeps_previous_route_unless_much_faster() {
        let probes = set(probe("200", 100), Some(probe("200", 90)), None);
        let previous = RouteDecision {
            channel: AutoChannel::Direct,
            available: true,
            latency_ms: 100,
            direct: probe("200", 100),
            wireguard: Some(probe("200", 90)),
            vless: None,
            via_zapret: false,
            checked_at: 0,
        };
        let choice = choose(Some(&previous), &probes).unwrap();
        assert_eq!(choice.channel, AutoChannel::Direct);
    }

    #[test]
    fn switches_when_previous_route_fails() {
        let probes = set(probe("000", 0), Some(probe("200", 150)), None);
        let previous = RouteDecision {
            channel: AutoChannel::Direct,
            available: true,
            latency_ms: 100,
            direct: probe("200", 100),
            wireguard: Some(probe("200", 150)),
            vless: None,
            via_zapret: false,
            checked_at: 0,
        };
        let choice = choose(Some(&previous), &probes).unwrap();
        assert_eq!(choice.channel, AutoChannel::Wireguard);
    }

    #[test]
    fn merges_forced_lists_and_keeps_direct_exceptions() {
        let base = std::env::temp_dir().join(format!(
            "wpc-route-migrate-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&base).unwrap();
        let list = |items: &[&str]| {
            items
                .iter()
                .map(|item| item.to_string())
                .collect::<Vec<_>>()
        };
        crate::lists::write_domain_file(&base, "extra-domains.txt", &list(&["example.com"]))
            .unwrap();
        crate::lists::write_domain_file(
            &base,
            "vless-domains.txt",
            &list(&["vless.test", "example.com"]),
        )
        .unwrap();
        crate::lists::write_domain_file(&base, "wg-domains.txt", &list(&["wg.test", "lan.test"]))
            .unwrap();
        crate::lists::write_domain_file(&base, "direct-domains.txt", &list(&["lan.test"])).unwrap();

        let extra = merge_forced_lists(&base).unwrap().unwrap();
        assert_eq!(extra, vec!["example.com", "vless.test", "wg.test"]);
        assert!(!base.join("vless-domains.txt").exists());
        assert!(!base.join("wg-domains.txt").exists());
        assert!(base.join("vless-domains.txt.migrated").exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
