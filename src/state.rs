//! Общее состояние приложения.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use std::time::Instant;

use chrono::{DateTime, Utc};

use crate::engines::Engine;
use crate::settings::Settings;
use crate::vless::pool::PoolEntry;

#[derive(Default, Clone)]
pub struct Health {
    pub wg_ok: bool,
    pub vless_ok: bool,
    pub exit_ip: Option<String>,
    pub exit_ip_vless: Option<String>,
    pub at: Option<DateTime<Utc>>,
    pub wg_fail: u8,
    pub vless_fail: u8,
}

#[derive(Default)]
pub struct Pool {
    pub testing: bool,
    pub updated_at: Option<DateTime<Utc>>,
    pub error: Option<String>,
    pub entries: Vec<PoolEntry>,
    pub active_link: Option<String>,
    pub tried: std::collections::HashSet<String>,
    pub rotate_cycle_at: Option<DateTime<Utc>>,
    pub last_rotate_at: Option<DateTime<Utc>>,
    pub last_attempt_at: Option<DateTime<Utc>>,
}

#[derive(Default)]
pub struct Lists {
    pub proxied: std::collections::HashSet<String>,
    pub direct: std::collections::HashSet<String>,
    pub last_update: Option<DateTime<Utc>>,
    pub error: Option<String>,
    pub refreshing: bool,
}

/// Прогресс скачивания движка (wireproxy/xray).
#[derive(Clone, Default, serde::Serialize)]
pub struct DownloadInfo {
    pub stage: String,
    pub done: u64,
    pub total: u64,
    pub error: Option<String>,
}

#[derive(Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckEntry {
    pub domain: String,
    pub direct: String,
    pub vpn: String,
    pub zapret: String,
    pub zapret_running: bool,
    pub zapret_supported: bool,
    pub verdict: String,
    pub at: String,
}

pub struct AppState {
    pub dir: PathBuf,
    pub panel_port: u16,
    /// Интерактивный запуск (не автозапуск): можно открыть браузер с панелью.
    pub interactive: bool,
    pub local_domains: Mutex<Vec<crate::net::local_proxy::LocalDomain>>,
    pub local_http_error: Mutex<Option<String>>,
    pub local_https_error: Mutex<Option<String>>,
    pub downloads: Mutex<HashMap<String, DownloadInfo>>,
    /// Умный прокси реально слушает 127.0.0.1:8081.
    pub smart_proxy_ready: AtomicBool,
    /// Идёт автоподбор самого быстрого VPN-конфига.
    pub config_auto_running: AtomicBool,
    /// Состояние простого режима (включение одной кнопкой).
    pub simple: Mutex<crate::simple::SimpleState>,
    pub simple_running: AtomicBool,
    pub tls_config:
        Mutex<Option<std::sync::Arc<tokio::sync::RwLock<std::sync::Arc<rustls::ServerConfig>>>>>,
    pub local_proxy_stop: Mutex<Option<(u64, tokio::sync::watch::Sender<bool>)>>,
    pub settings: Mutex<Settings>,
    /// User-owned domain lists, loaded from SQLite and kept in sync by the
    /// API. Remote auto-lists stay in `Lists` because they are re-downloaded.
    pub domains: Mutex<crate::store::DomainSets>,
    pub wg: Engine,
    pub xr: Engine,
    pub telegram: crate::telegram::TelegramRuntime,
    pub zapret_error: Mutex<Option<String>>,
    pub zapret_running: Mutex<Option<(bool, Instant)>>,
    pub zapret_auto: Mutex<crate::zapret::AutoTuneState>,
    pub direct_probe: Mutex<HashMap<String, (bool, Instant)>>,
    pub health: Mutex<Health>,
    pub auto_routes: Mutex<crate::route_auto::AutoRoutes>,
    pub lists: Mutex<Lists>,
    pub pool: Mutex<Pool>,
    /// Serializes pool-state snapshots so concurrent refresh/apply cannot
    /// overwrite a newer active link with an older snapshot.
    pub pool_save: Mutex<()>,
    pub store: crate::store::Store,
    pub http_clients: Mutex<HashMap<String, reqwest::Client>>,
}

impl AppState {
    pub fn new(
        dir: PathBuf,
        panel_port: u16,
        interactive: bool,
    ) -> anyhow::Result<std::sync::Arc<Self>> {
        crate::engines::set_data_dir(&dir);
        let store = crate::store::Store::open(&dir)
            .map_err(|error| error.context("не удалось открыть wireproxy.db"))?;
        crate::storage::legacy::import_domains(&dir, &store)
            .map_err(|error| error.context("миграция legacy-доменов"))?;
        crate::storage::legacy::import_wg_profiles(&dir, &store)
            .map_err(|error| error.context("миграция WireGuard-профилей"))?;
        crate::storage::legacy::import_xray_profiles(&dir, &store)
            .map_err(|error| error.context("миграция Xray-профилей"))?;
        crate::storage::legacy::import_pool_state(&dir, &store)
            .map_err(|error| error.context("миграция состояния пула"))?;
        let settings =
            Settings::load(&store, &dir).map_err(|error| error.context("загрузка настроек"))?;
        let wg = Engine::new(crate::engines::EngineKind::Wireproxy, {
            let d = dir.clone();
            move || crate::engines::wireproxy::prepare(&d)
        });
        let xr = Engine::new(crate::engines::EngineKind::Xray, {
            let d = dir.clone();
            move || crate::engines::xray::has_config(&d)
        });
        let local_domains = store.local_domains().unwrap_or_default();
        let domains = store.domain_sets().unwrap_or_default();
        crate::route_auto::import_legacy_json(&dir, &store);
        crate::storage::materialize::reconcile(&store, &dir, &local_domains)
            .map_err(|error| error.context("материализация конфигов движков"))?;
        let auto_routes = crate::route_auto::AutoRoutes::from_store(&store);
        let st = std::sync::Arc::new(Self {
            dir,
            panel_port,
            interactive,
            local_domains: Mutex::new(local_domains),
            local_http_error: Mutex::new(None),
            local_https_error: Mutex::new(None),
            downloads: Mutex::new(HashMap::new()),
            smart_proxy_ready: AtomicBool::new(false),
            config_auto_running: AtomicBool::new(false),
            simple: Mutex::new(crate::simple::SimpleState::default()),
            simple_running: AtomicBool::new(false),
            tls_config: Mutex::new(None),
            local_proxy_stop: Mutex::new(None),
            settings: Mutex::new(settings),
            domains: Mutex::new(domains),
            wg,
            xr,
            telegram: crate::telegram::TelegramRuntime::default(),
            zapret_error: Mutex::new(None),
            zapret_running: Mutex::new(None),
            zapret_auto: Mutex::new(crate::zapret::AutoTuneState::default()),
            direct_probe: Mutex::new(HashMap::new()),
            health: Mutex::new(Health::default()),
            auto_routes: Mutex::new(auto_routes),
            lists: Mutex::new(Lists::default()),
            pool: Mutex::new(Pool::default()),
            pool_save: Mutex::new(()),
            store,
            http_clients: Mutex::new(HashMap::new()),
        });
        crate::vless::pool::load_pool_state(&st);
        Ok(st)
    }

    pub fn settings_snapshot(&self) -> Settings {
        self.settings.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Applies a settings change under the settings lock and persists the new
    /// aggregate in one transaction. Callers must not keep the old snapshot
    /// and save it later.
    pub fn update_settings<F>(&self, change: F) -> anyhow::Result<Settings>
    where
        F: FnOnce(&mut Settings),
    {
        let mut guard = self
            .settings
            .lock()
            .map_err(|_| anyhow::anyhow!("блокировка настроек повреждена"))?;
        let mut next = guard.clone();
        change(&mut next);
        next.save_to_store(&self.store)?;
        *guard = next.clone();
        Ok(next)
    }

    pub fn domain_sets_snapshot(&self) -> crate::store::DomainSets {
        self.domains.lock().map(|d| d.clone()).unwrap_or_default()
    }

    pub fn mutate_domain_sets<F>(&self, change: F) -> anyhow::Result<crate::store::DomainSets>
    where
        F: FnOnce(&mut crate::store::DomainSets),
    {
        let sets = self.store.mutate_domain_sets(change)?;
        if let Ok(mut cached) = self.domains.lock() {
            *cached = sets.clone();
        }
        Ok(sets)
    }

    pub fn set_local_domains(
        &self,
        list: &[crate::net::local_proxy::LocalDomain],
    ) -> anyhow::Result<()> {
        self.store.replace_local_domains(list)?;
        if let Ok(mut cached) = self.local_domains.lock() {
            *cached = list.to_vec();
        }
        Ok(())
    }

    pub fn health_snapshot(&self) -> (bool, bool) {
        self.health
            .lock()
            .map(|h| (h.wg_ok, h.vless_ok))
            .unwrap_or((false, false))
    }

    /// Снимок маршрутизации для текущего запроса.
    pub fn routing_snapshot(&self) -> crate::routing::Routing {
        let settings = self.settings_snapshot();
        let (wg_ok, vless_ok) = self.health_snapshot();
        let lists = self.lists.lock().ok();
        let (proxied, direct) = match &lists {
            Some(l) => (l.proxied.clone(), l.direct.clone()),
            None => Default::default(),
        };
        let auto = self
            .auto_routes
            .lock()
            .map(|routes| {
                routes
                    .routes
                    .iter()
                    .map(|(domain, decision)| {
                        (
                            domain.clone(),
                            crate::routing::AutoChoice {
                                channel: decision.channel,
                                via_zapret: decision.via_zapret,
                                vpn: decision.fastest_vpn(),
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let vpn_only = self
            .domains
            .lock()
            .map(|domains| domains.vpn_only.iter().cloned().collect())
            .unwrap_or_default();
        crate::routing::Routing {
            proxied,
            direct_override: direct,
            vpn_only,
            auto,
            vpn_default: if settings.vpn_default == "vless" {
                crate::routing::Channel::Vless
            } else {
                crate::routing::Channel::Wireguard
            },
            wg_healthy: wg_ok,
            vless_healthy: vless_ok,
        }
    }

    /// Кэшированный HTTP-клиент (direct или через локальный upstream-прокси).
    pub fn client(&self, key: &str, proxy_port: Option<u16>) -> reqwest::Client {
        if let Ok(map) = self.http_clients.lock() {
            if let Some(c) = map.get(key) {
                return c.clone();
            }
        }
        let mut b = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .user_agent("wireproxy-control/1.0");
        if let Some(port) = proxy_port {
            if let Ok(proxy) = reqwest::Proxy::all(format!("http://127.0.0.1:{port}")) {
                b = b.proxy(proxy);
            }
        }
        let client = b.build().unwrap_or_else(|_| reqwest::Client::new());
        if let Ok(mut map) = self.http_clients.lock() {
            map.insert(key.to_string(), client.clone());
        }
        client
    }
}

/// Клиент для health-проб: без переиспользования idle-соединений.
/// wireproxy/xray закрывают простаивающие соединения, и кэшированный клиент
/// даёт ложные «канал не проходит трафик» через одну пробу.
pub fn probe_client(proxy_port: u16) -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent("wireproxy-control/1.0")
        .pool_max_idle_per_host(0);
    if let Ok(proxy) = reqwest::Proxy::all(format!("http://127.0.0.1:{proxy_port}")) {
        builder = builder.proxy(proxy);
    }
    builder.build().unwrap_or_else(|_| reqwest::Client::new())
}
