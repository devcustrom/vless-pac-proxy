//! Общее состояние приложения.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

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

#[derive(Default)]
pub struct Visited {
    pub items: Vec<(String, u64, i64)>, // domain, count, last_ts
}

impl Visited {
    pub fn note(&mut self, host: &str) {
        let now = chrono::Utc::now().timestamp();
        if let Some(pos) = self.items.iter().position(|(d, _, _)| d == host) {
            let (d, c, _) = self.items.remove(pos);
            self.items.push((d, c + 1, now));
            return;
        }
        self.items.push((host.to_string(), 1, now));
        if self.items.len() > 300 {
            self.items.remove(0);
        }
    }
}

#[derive(Clone, Default, serde::Serialize)]
pub struct CheckEntry {
    pub domain: String,
    pub direct: String,
    pub vpn: String,
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
    pub tls_config:
        Mutex<Option<std::sync::Arc<tokio::sync::RwLock<std::sync::Arc<rustls::ServerConfig>>>>>,
    pub settings: Mutex<Settings>,
    pub wg: Engine,
    pub xr: Engine,
    pub health: Mutex<Health>,
    pub lists: Mutex<Lists>,
    pub pool: Mutex<Pool>,
    pub visited: Mutex<Visited>,
    pub checks: Mutex<Vec<CheckEntry>>,
    pub http_clients: Mutex<HashMap<String, reqwest::Client>>,
}

impl AppState {
    pub fn new(dir: PathBuf, panel_port: u16, interactive: bool) -> std::sync::Arc<Self> {
        crate::engines::set_data_dir(&dir);
        let settings = Settings::load(&dir);
        let wg = Engine::new(crate::engines::EngineKind::Wireproxy, {
            let d = dir.clone();
            move || crate::engines::wireproxy::is_configured(&d)
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
        let st = std::sync::Arc::new(Self {
            dir,
            panel_port,
            interactive,
            local_domains: Mutex::new(local_domains),
            local_http_error: Mutex::new(None),
            local_https_error: Mutex::new(None),
            downloads: Mutex::new(HashMap::new()),
            tls_config: Mutex::new(None),
            settings: Mutex::new(settings),
            wg,
            xr,
            health: Mutex::new(Health::default()),
            lists: Mutex::new(Lists::default()),
            pool: Mutex::new(Pool::default()),
            visited: Mutex::new(Visited::default()),
            checks: Mutex::new(Vec::new()),
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
        let (proxied, direct, vless_only) = match &lists {
            Some(l) => (l.proxied.clone(), l.direct.clone(), l.vless_only.clone()),
            None => Default::default(),
        };
        crate::routing::Routing {
            proxied,
            direct_override: direct,
            vless_only,
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
