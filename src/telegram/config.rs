use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::settings::Settings;

pub const STATUS_FILE: &str = "tgws-status.json";
pub const CONFIG_FILE: &str = "tgws-config.json";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramConfig {
    pub host: String,
    pub port: u16,
    pub secret: String,
    pub dc_redirects: BTreeMap<String, String>,
    /// Отдельная карта для медиа: upstream рекомендует не направлять
    /// медиа-DC2 на общий WS-edge, иначе фото/видео зависают.
    pub media_dc_redirects: BTreeMap<String, String>,
    pub buffer_size: u32,
    pub pool_size: u32,
    pub fallback_cfproxy: bool,
    pub cfproxy_user_domains: Vec<String>,
    pub cfproxy_worker_domains: Vec<String>,
    pub cfproxy_h2_media: bool,
    pub disable_secure: bool,
    pub fake_tls_domain: String,
    pub proxy_protocol: bool,
    pub force_test_dc: bool,
    pub managed: bool,
    pub status_file: String,
    pub upstream_order: Vec<String>,
    pub wg_proxy: String,
    pub vless_proxy: String,
    pub upstream_timeout: f64,
    pub verbose: bool,
    pub parent_pid: u32,
}

pub fn generate_secret() -> String {
    use rand::rngs::OsRng;
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}

pub fn valid_secret(secret: &str) -> bool {
    secret.len() == 32 && secret.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn mask_secret(secret: &str) -> String {
    if secret.len() < 8 {
        return "***".to_string();
    }
    format!("{}…{}", &secret[..4], &secret[secret.len() - 4..])
}

/// Разбирает домены из строки, разделённой запятыми/пробелами/точками с запятой.
pub fn split_domains(raw: &str) -> Vec<String> {
    raw.split(|c: char| c == ',' || c == ';' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

pub fn validate_domains(raw: &str) -> Result<Vec<String>> {
    let mut result = Vec::new();
    for item in split_domains(raw) {
        let Some(domain) = crate::lists::validate_hostname(&item) else {
            bail!("некорректный домен: {item}");
        };
        if !result.contains(&domain) {
            result.push(domain);
        }
    }
    Ok(result)
}

fn reserved_port(port: u16, panel_port: u16) -> bool {
    matches!(
        port,
        80 | 443 | 5173 | 8080 | 8081 | 25344 | 25345 | 25346 | 26249 | 26250
    ) || port == panel_port
}

pub fn order_for(settings: &Settings) -> Result<Vec<String>> {
    let try_direct = settings.telegram_try_direct;
    let preset = settings.telegram_order.trim().to_ascii_lowercase();
    let mut order: Vec<String> = match preset.as_str() {
        "" | "auto" => {
            if try_direct {
                vec!["direct".into(), "wg".into(), "vless".into()]
            } else {
                vec!["wg".into(), "vless".into()]
            }
        }
        "vpn-first" => {
            if try_direct {
                vec!["wg".into(), "vless".into(), "direct".into()]
            } else {
                vec!["wg".into(), "vless".into()]
            }
        }
        "direct-only" => {
            if try_direct {
                vec!["direct".into()]
            } else {
                bail!("direct-only выбран, но «пробовать напрямую» выключено");
            }
        }
        "vpn-only" => vec!["wg".into(), "vless".into()],
        custom => custom
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
    };
    order.retain(|item| matches!(item.as_str(), "direct" | "wg" | "vless"));
    if !try_direct {
        order.retain(|item| item != "direct");
    }
    let mut deduped = Vec::new();
    for item in order {
        if !deduped.contains(&item) {
            deduped.push(item);
        }
    }
    if deduped.is_empty() {
        bail!("нужен хотя бы один исходящий путь (direct, wg, vless)");
    }
    Ok(deduped)
}

impl TelegramConfig {
    pub fn from_settings(settings: &Settings, panel_port: u16) -> Result<Self> {
        if settings.telegram_host != "127.0.0.1" {
            bail!("Telegram-прокси в v1 слушает только 127.0.0.1");
        }
        if settings.telegram_port == 0 || reserved_port(settings.telegram_port, panel_port) {
            bail!(
                "порт {} занят или зарезервирован; выберите другой",
                settings.telegram_port
            );
        }
        if !valid_secret(&settings.telegram_secret) {
            bail!("секрет Telegram-прокси не сгенерирован");
        }
        let cfproxy = validate_domains(&settings.telegram_cf_proxy)?;
        let cfworker = validate_domains(&settings.telegram_cf_worker)?;
        let fake = validate_domains(&settings.telegram_fake_tls)?;
        let fake_tls_domain = fake.first().cloned().unwrap_or_default();
        let order = order_for(settings)?;
        let h2 = settings.telegram_h2 && order == ["direct"];
        // Обычные соединения DC2/DC4 держим на WS-edge — иначе почти всё
        // уходит в TCP fallback и Telegram теряет соединения. Для медиа
        // upstream рекомендует только DC4: redirect DC2 ломает фото/видео
        // на аккаунтах без Premium.
        let mut dc_redirects = BTreeMap::new();
        dc_redirects.insert("2".to_string(), "149.154.167.220".to_string());
        dc_redirects.insert("4".to_string(), "149.154.167.220".to_string());
        // DC203 в Telegram использует те же kws2-эндпоинты; без него
        // большинство соединений уходило в TCP fallback.
        dc_redirects.insert("203".to_string(), "149.154.167.220".to_string());
        let mut media_dc_redirects = BTreeMap::new();
        media_dc_redirects.insert("4".to_string(), "149.154.167.220".to_string());
        Ok(Self {
            host: "127.0.0.1".to_string(),
            port: settings.telegram_port,
            secret: settings.telegram_secret.clone(),
            dc_redirects,
            media_dc_redirects,
            buffer_size: 256 * 1024,
            pool_size: 4,
            fallback_cfproxy: !cfproxy.is_empty() || !cfworker.is_empty(),
            cfproxy_user_domains: cfproxy,
            cfproxy_worker_domains: cfworker,
            cfproxy_h2_media: h2,
            disable_secure: false,
            fake_tls_domain,
            proxy_protocol: false,
            force_test_dc: false,
            managed: true,
            status_file: STATUS_FILE.to_string(),
            upstream_order: order,
            wg_proxy: format!("127.0.0.1:{}", crate::vless::WIREPROXY_HTTP_PORT),
            vless_proxy: format!("127.0.0.1:{}", crate::vless::XRAY_HTTP_PORT),
            upstream_timeout: 8.0,
            verbose: settings.telegram_verbose,
            parent_pid: std::process::id(),
        })
    }

    pub fn write(&self, dir: &Path) -> Result<std::path::PathBuf> {
        let path = crate::paths::in_dir(dir, CONFIG_FILE)?;
        let text = serde_json::to_string_pretty(self)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text).context("запись tgws-config.json")?;
        std::fs::rename(&tmp, &path).context("переименование tgws-config.json")?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(order: &str, try_direct: bool) -> Settings {
        Settings {
            telegram_order: order.to_string(),
            telegram_try_direct: try_direct,
            telegram_secret: "0123456789abcdef0123456789abcdef".to_string(),
            ..Settings::default()
        }
    }

    #[test]
    fn order_presets() {
        assert_eq!(
            order_for(&settings("auto", true)).unwrap(),
            ["direct", "wg", "vless"]
        );
        assert_eq!(
            order_for(&settings("auto", false)).unwrap(),
            ["wg", "vless"]
        );
        assert_eq!(
            order_for(&settings("vpn-first", true)).unwrap(),
            ["wg", "vless", "direct"]
        );
        assert_eq!(
            order_for(&settings("direct-only", true)).unwrap(),
            ["direct"]
        );
        assert!(order_for(&settings("direct-only", false)).is_err());
        assert_eq!(
            order_for(&settings("vless,direct,vless", true)).unwrap(),
            ["vless", "direct"]
        );
    }

    #[test]
    fn media_redirects_are_separate_from_normal_ws() {
        let cfg = TelegramConfig::from_settings(&settings("vpn-first", true), 8080).unwrap();
        assert_eq!(
            cfg.dc_redirects.get("2").map(String::as_str),
            Some("149.154.167.220")
        );
        assert_eq!(
            cfg.dc_redirects.get("4").map(String::as_str),
            Some("149.154.167.220")
        );
        assert_eq!(
            cfg.dc_redirects.get("203").map(String::as_str),
            Some("149.154.167.220")
        );
        assert_eq!(cfg.media_dc_redirects.len(), 1);
        assert_eq!(
            cfg.media_dc_redirects.get("4").map(String::as_str),
            Some("149.154.167.220")
        );
        assert!(!cfg.media_dc_redirects.contains_key("2"));
    }

    #[test]
    fn secret_and_mask() {
        let s = generate_secret();
        assert!(valid_secret(&s));
        assert!(mask_secret(&s).contains('…'));
        assert!(!mask_secret(&s).contains(&s));
    }

    #[test]
    fn config_rejects_reserved_port_and_bad_domain() {
        let mut s = settings("auto", true);
        s.telegram_port = 8081;
        assert!(TelegramConfig::from_settings(&s, 8080).is_err());
        s.telegram_port = 15443;
        s.telegram_cf_proxy = "not a domain".to_string();
        assert!(TelegramConfig::from_settings(&s, 8080).is_err());
    }
}
