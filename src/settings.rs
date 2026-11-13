//! settings.json — байт-совместимый с legacy-версией формат (5 полей).
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

impl Default for Settings {
    fn default() -> Self {
        Self {
            vpn_default: default_vpn(),
            auto_rotate: true,
            auto_pool_refresh_hours: default_hours(),
            local_proxy_port: default_http_port(),
            local_https_port: default_https_port(),
        }
    }
}

impl Settings {
    pub fn load(dir: &Path) -> Self {
        let path = match paths::in_dir(dir, "settings.json") {
            Ok(p) => p,
            Err(_) => return Self::default(),
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<Settings>(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
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
    }
}
