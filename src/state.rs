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
    pub vless_only: std::collections::HashSet<String>,
    pub wg_only: std::collections::HashSet<String>,
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
    pub tls_config:
        Mutex<Option<std::sync::Arc<tokio::sync::RwLock<std::sync::Arc<rustls::ServerConfig>>>>>,
    pub local_proxy_stop: Mutex<Option<(u64, tokio::sync::watch::Sender<bool>)>>,
    pub settings: Mutex<Settings>,
    pub wg: Engine,
    pub xr: Engine,
    pub telegram: crate::telegram::TelegramRuntime,
    pub zapret_error: Mutex<Option<String>>,
    pub zapret_running: Mutex<Option<(bool, Instant)>>,
    pub zapret_auto: Mutex<crate::zapret::AutoTuneState>,
    pub direct_probe: Mutex<HashMap<String, (bool, Instant)>>,
    pub health: Mutex<Health>,
    pub lists: Mutex<Lists>,
    pub pool: Mutex<Pool>,
    pub store: crate::store::Store,
    pub http_clients: Mutex<HashMap<String, reqwest::Client>>,
}

impl AppState {
    pub fn new(dir: PathBuf, panel_port: u16, interactive: bool) -> std::sync::Arc<Self> {
        crate::engines::set_data_dir(&dir);
        let settings = Settings::load(&dir);
        let wg = Engine::new(crate::engines::EngineKind::Wireproxy, {
            let d = dir.clone();
            move || crate::engines::wireproxy::prepare(&d)
        });
        let xr = Engine::new(crate::engines::EngineKind::Xray, {
            let d = dir.clone();
            move || crate::engines::xray::has_config(&d)
        });
        let local_domains = crate::paths::in_dir(&dir, "local-domains.txt")
            .ok()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|t| crate::net::local_proxy::parse_config(&t))
            .unwrap_or_default();
        let store = match crate::store::Store::open(&dir) {
            Ok(store) => store,
            Err(e) => {
                crate::wlog!("SQLite недоступен ({e:#}); история только в памяти");
                crate::store::Store::memory()
            }
        };
        let st = std::sync::Arc::new(Self {
            dir,
            panel_port,
            interactive,
            local_domains: Mutex::new(local_domains),
            local_http_error: Mutex::new(None),
            local_https_error: Mutex::new(None),
            downloads: Mutex::new(HashMap::new()),
            smart_proxy_ready: AtomicBool::new(false),
            tls_config: Mutex::new(None),
            local_proxy_stop: Mutex::new(None),
            settings: Mutex::new(settings),
            wg,
            xr,
            telegram: crate::telegram::TelegramRuntime::default(),
            zapret_error: Mutex::new(None),
            zapret_running: Mutex::new(None),
            zapret_auto: Mutex::new(crate::zapret::AutoTuneState::default()),
            direct_probe: Mutex::new(HashMap::new()),
            health: Mutex::new(Health::default()),
            lists: Mutex::new(Lists::default()),
            pool: Mutex::new(Pool::default()),
            store,
            http_clients: Mutex::new(HashMap::new()),
        });
        crate::vless::pool::load_pool_state(&st);
        st
    }

    pub fn settings_snapshot(&self) -> Settings {
        self.settings.lock().map(|s| s.clone()).unwrap_or_default()
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
        let (proxied, direct, vless_only, wg_only) = match &lists {
            Some(l) => (
                l.proxied.clone(),
                l.direct.clone(),
                l.vless_only.clone(),
                l.wg_only.clone(),
            ),
            None => Default::default(),
        };
        crate::routing::Routing {
            proxied,
            direct_override: direct,
            vless_only,
            wg_only,
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
