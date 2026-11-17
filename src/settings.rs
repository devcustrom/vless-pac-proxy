//! settings.json — совместимый с legacy-версией формат; новые поля имеют
//! значения по умолчанию и не ломают старые файлы.
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(rename = "vpnDefault", default = "default_vpn")]
    pub vpn_default: String,
    #[serde(rename = "autoRotate", default = "default_true")]
    pub auto_rotate: bool,
    #[serde(rename = "autoPoolRefreshHours", default = "default_hours")]
    pub auto_pool_refresh_hours: f64,
    #[serde(rename = "localProxyPort", default = "default_http_port")]
    pub local_proxy_port: u16,
    #[serde(rename = "localHttpsPort", default = "default_https_port")]
    pub local_https_port: u16,
    #[serde(rename = "localDomainsEnabled", default)]
    pub local_domains_enabled: bool,
    #[serde(rename = "wireguardEnabled", default = "default_true")]
    pub wireguard_enabled: bool,
    #[serde(rename = "xrayEnabled", default = "default_true")]
    pub xray_enabled: bool,
    #[serde(rename = "onboardingDone", default)]
    pub onboarding_done: bool,
    #[serde(rename = "telegramEnabled", default)]
    pub telegram_enabled: bool,
    #[serde(rename = "telegramHost", default = "default_telegram_host")]
    pub telegram_host: String,
    #[serde(rename = "telegramPort", default = "default_telegram_port")]
    pub telegram_port: u16,
    #[serde(rename = "telegramSecret", default)]
    pub telegram_secret: String,
    #[serde(rename = "telegramTryDirect", default = "default_true")]
    pub telegram_try_direct: bool,
    #[serde(rename = "telegramOrder", default = "default_telegram_order")]
    pub telegram_order: String,
    #[serde(rename = "telegramCfWorker", default)]
    pub telegram_cf_worker: String,
    #[serde(rename = "telegramCfProxy", default)]
    pub telegram_cf_proxy: String,
    #[serde(rename = "telegramFakeTls", default)]
    pub telegram_fake_tls: String,
    #[serde(rename = "telegramH2", default)]
    pub telegram_h2: bool,
    #[serde(rename = "telegramVerbose", default)]
    pub telegram_verbose: bool,
    #[serde(rename = "telegramAutoUpdate", default = "default_true")]
    pub telegram_auto_update: bool,
    #[serde(rename = "zapretEnabled", default)]
    pub zapret_enabled: bool,
    #[serde(rename = "zapretPreset", default = "default_zapret_preset")]
    pub zapret_preset: String,
    #[serde(rename = "directFirst", default = "default_true")]
    pub direct_first: bool,
}

fn default_vpn() -> String {
    "wireguard".to_string()
}
fn default_true() -> bool {
    true
}
fn default_hours() -> f64 {
    6.0
}
fn default_http_port() -> u16 {
    80
}
fn default_https_port() -> u16 {
    443
}
fn default_telegram_host() -> String {
    "127.0.0.1".to_string()
}
fn default_telegram_port() -> u16 {
    1443
}
fn default_telegram_order() -> String {
    "auto".to_string()
}
fn default_zapret_preset() -> String {
    "general.bat".to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            vpn_default: default_vpn(),
            auto_rotate: true,
            auto_pool_refresh_hours: default_hours(),
            local_proxy_port: default_http_port(),
            local_https_port: default_https_port(),
            local_domains_enabled: false,
            wireguard_enabled: true,
            xray_enabled: true,
            onboarding_done: false,
            telegram_enabled: false,
            telegram_host: default_telegram_host(),
            telegram_port: default_telegram_port(),
            telegram_secret: String::new(),
            telegram_try_direct: true,
            telegram_order: default_telegram_order(),
            telegram_cf_worker: String::new(),
            telegram_cf_proxy: String::new(),
            telegram_fake_tls: String::new(),
            telegram_h2: false,
            telegram_verbose: false,
            telegram_auto_update: true,
            zapret_enabled: false,
            zapret_preset: default_zapret_preset(),
            direct_first: true,
        }
    }
}

impl Settings {
    pub fn load(dir: &Path) -> Self {
        let path = match paths::in_dir(dir, "settings.json") {
            Ok(p) => p,
            Err(_) => return Self::default(),
        };
        let mut settings = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<Settings>(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        };
        // Старые установки с уже включённым плагином не должны получать
        // мастер первого запуска после обновления.
        if settings.local_domains_enabled && !settings.onboarding_done {
            settings.onboarding_done = true;
            let _ = settings.save(dir);
        }
        settings
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        let path = paths::in_dir(dir, "settings.json")?;
        let text = serde_json::to_string_pretty(self)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    pub fn valid(&self) -> bool {
        (self.vpn_default == "wireguard" || self.vpn_default == "vless")
            && (0.0..=168.0).contains(&self.auto_pool_refresh_hours)
            && self.local_proxy_port > 0
            && self.local_https_port > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let s = Settings {
            vpn_default: "vless".into(),
            auto_rotate: false,
            auto_pool_refresh_hours: 1.0,
            local_proxy_port: 8080,
            local_https_port: 8443,
            local_domains_enabled: false,
            wireguard_enabled: true,
            xray_enabled: true,
            onboarding_done: false,
            telegram_enabled: false,
            telegram_host: default_telegram_host(),
            telegram_port: default_telegram_port(),
            telegram_secret: String::new(),
            telegram_try_direct: true,
            telegram_order: default_telegram_order(),
            telegram_cf_worker: String::new(),
            telegram_cf_proxy: String::new(),
            telegram_fake_tls: String::new(),
            telegram_h2: false,
            telegram_verbose: false,
            telegram_auto_update: true,
            zapret_enabled: false,
            zapret_preset: default_zapret_preset(),
            direct_first: true,
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"vpnDefault\":\"vless\""));
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.vpn_default, "vless");
        assert!(!back.auto_rotate);
    }

    #[test]
    fn defaults_on_bad_file() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.vpn_default, "wireguard");
        assert!(s.auto_rotate);
        assert_eq!(s.local_proxy_port, 80);
        assert!(!s.local_domains_enabled);
        assert!(s.wireguard_enabled);
        assert!(s.xray_enabled);
    }
}
