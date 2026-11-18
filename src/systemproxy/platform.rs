//! Платформенные реализации системного прокси.
use std::path::Path;

use anyhow::Result;

pub const PROXY_ADDR: &str = "127.0.0.1:8081";
pub const PROXY_OVERRIDE: &str =
    "localhost;127.0.0.1;*.localhost;*.local;192.168.*;10.*;172.16.*;<local>;zcode.z.ai;*.z.ai";

/// Базовые исключения + домены локального плагина: их нельзя отправлять
/// в системный прокси, они живут на loopback.
pub fn override_list(local_domains: &[String]) -> String {
    let mut items: Vec<String> = PROXY_OVERRIDE
        .split(';')
        .filter(|item| !item.is_empty())
        .map(ToString::to_string)
        .collect();
    for domain in local_domains {
        let domain = domain.trim();
        if domain.is_empty() {
            continue;
        }
        if !items.iter().any(|item| item.eq_ignore_ascii_case(domain)) {
            items.push(domain.to_string());
        }
    }
    items.join(";")
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ProxyState {
    pub enabled: bool,
    pub mode: Option<String>,
    pub value: Option<String>,
}

#[cfg(windows)]
mod imp {
    use anyhow::Result;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_ALL_ACCESS, KEY_READ};
    use winreg::RegKey;

    use super::{override_list, ProxyState, PROXY_ADDR};

    const KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

    #[link(name = "wininet")]
    extern "system" {
        fn InternetSetOptionW(
            hinternet: *mut core::ffi::c_void,
            dwoption: u32,
            lpbuffer: *mut core::ffi::c_void,
            dwbufferlength: u32,
        ) -> i32;
    }

    fn refresh_wininet() {
        // 39 = INTERNET_OPTION_SETTINGS_CHANGED, 37 = INTERNET_OPTION_REFRESH
        unsafe {
            InternetSetOptionW(std::ptr::null_mut(), 39, std::ptr::null_mut(), 0);
            InternetSetOptionW(std::ptr::null_mut(), 37, std::ptr::null_mut(), 0);
        }
    }

    pub fn set_system_proxy(enabled: bool, local_domains: &[String]) -> Result<ProxyState> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = hkcu.open_subkey_with_flags(KEY, KEY_ALL_ACCESS)?;
        if enabled {
            key.set_value("ProxyEnable", &1u32)?;
            key.set_value("ProxyServer", &PROXY_ADDR)?;
            key.set_value("ProxyOverride", &override_list(local_domains))?;
            let _ = key.delete_value("AutoConfigURL");
        } else {
            key.set_value("ProxyEnable", &0u32)?;
        }
        refresh_wininet();
        get_system_proxy()
    }

    pub fn get_system_proxy() -> Result<ProxyState> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = hkcu.open_subkey_with_flags(KEY, KEY_READ)?;
        let enabled: u32 = key.get_value("ProxyEnable").unwrap_or(0);
        let server: Option<String> = key.get_value("ProxyServer").ok();
        let pac: Option<String> = key.get_value("AutoConfigURL").ok();
        if enabled == 1 && server.as_deref() == Some(PROXY_ADDR) {
            return Ok(ProxyState {
                enabled: true,
                mode: Some("static".into()),
                value: server,
            });
        }
        if let Some(url) = pac {
            return Ok(ProxyState {
                enabled: false,
                mode: Some("pac".into()),
                value: Some(url),
            });
        }
        Ok(ProxyState {
            enabled: false,
            mode: None,
            value: server,
        })
    }

    /// Синхронное выключение при выходе (чтобы не сломать браузер).
    pub fn disable_on_exit() {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        if let Ok(key) = hkcu.open_subkey_with_flags(KEY, KEY_ALL_ACCESS) {
            let _ = key.set_value("ProxyEnable", &0u32);
        }
        refresh_wininet();
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use anyhow::{bail, Result};
    use std::process::Command;

    use super::{override_list, ProxyState, PROXY_ADDR};

    /// Активные сетевые сервисы (Wi-Fi, Ethernet…), кроме отключённых (`*`).
    fn services() -> Result<Vec<String>> {
        let out = Command::new("networksetup")
            .arg("-listallnetworkservices")
            .output()?;
        if !out.status.success() {
            bail!("networksetup: не удалось получить список сервисов");
        }
        let text = String::from_utf8_lossy(&out.stdout);
        Ok(text
            .lines()
            .skip(1)
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('*'))
            .map(ToString::to_string)
            .collect())
    }

    fn run(args: &[String]) -> Result<()> {
        let out = Command::new("networksetup").args(args).output()?;
        if !out.status.success() {
            bail!(
                "networksetup {}: {}",
                args.first().map(String::as_str).unwrap_or(""),
                String::from_utf8_lossy(&out.stderr).trim()
            );
        }
        Ok(())
    }

    fn points_to_wpc(service: &str, secure: bool) -> bool {
        enabled_endpoint(service, secure)
            .map(|(server, port)| server == "127.0.0.1" && port == "8081")
            .unwrap_or(false)
    }

    fn enabled_endpoint(service: &str, secure: bool) -> Option<(String, String)> {
        let flag = if secure {
            "-getsecurewebproxy"
        } else {
            "-getwebproxy"
        };
        let out = Command::new("networksetup")
            .args([flag, service])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let enabled = text
            .lines()
            .any(|line| line.trim().eq_ignore_ascii_case("Enabled: Yes"));
        if !enabled {
            return None;
        }
        let server = text
            .lines()
            .find_map(|line| line.trim().strip_prefix("Server: "))
            .unwrap_or("")
            .trim()
            .to_string();
        let port = text
            .lines()
            .find_map(|line| line.trim().strip_prefix("Port: "))
            .unwrap_or("")
            .trim()
            .to_string();
        Some((server, port))
    }

    fn has_foreign_proxy(service: &str) -> bool {
        [false, true].into_iter().any(|secure| {
            enabled_endpoint(service, secure)
                .map(|(server, port)| !(server == "127.0.0.1" && port == "8081"))
                .unwrap_or(false)
        })
    }

    pub fn set_system_proxy(enabled: bool, local_domains: &[String]) -> Result<ProxyState> {
        let (host, port) = match PROXY_ADDR.split_once(':') {
            Some((h, p)) => (h.to_string(), p.to_string()),
            None => (PROXY_ADDR.to_string(), "8081".to_string()),
        };
        for service in services()? {
            if enabled {
                if has_foreign_proxy(&service) {
                    // Не затираем чужой прокси (корпоративный/VPN-клиент).
                    continue;
                }
                run(&[
                    "-setwebproxy".into(),
                    service.clone(),
                    host.clone(),
                    port.clone(),
                ])?;
                run(&[
                    "-setsecurewebproxy".into(),
                    service.clone(),
                    host.clone(),
                    port.clone(),
                ])?;
                let mut bypass: Vec<String> =
                    vec!["-setproxybypassdomains".into(), service.clone()];
                bypass.extend(
                    override_list(local_domains)
                        .split(';')
                        .filter(|item| !item.is_empty() && *item != "<local>")
                        .map(ToString::to_string),
                );
                run(&bypass)?;
                run(&["-setwebproxystate".into(), service.clone(), "on".into()])?;
                run(&["-setsecurewebproxystate".into(), service, "on".into()])?;
            } else {
                // Не выключаем чужой корпоративный прокси: только наш 8081.
                if points_to_wpc(&service, false) {
                    run(&["-setwebproxystate".into(), service.clone(), "off".into()])?;
                }
                if points_to_wpc(&service, true) {
                    run(&["-setsecurewebproxystate".into(), service, "off".into()])?;
                }
            }
        }
        Ok(ProxyState {
            enabled,
            mode: Some("static".to_string()),
            value: Some(PROXY_ADDR.to_string()),
        })
    }

    pub fn get_system_proxy() -> Result<ProxyState> {
        for service in services().unwrap_or_default() {
            let out = Command::new("networksetup")
                .args(["-getwebproxy", &service])
                .output()?;
            let text = String::from_utf8_lossy(&out.stdout);
            let enabled = text
                .lines()
                .any(|line| line.trim().eq_ignore_ascii_case("Enabled: Yes"));
            if enabled {
                let server = text
                    .lines()
                    .find_map(|line| line.trim().strip_prefix("Server: "))
                    .unwrap_or("127.0.0.1");
                let port = text
                    .lines()
                    .find_map(|line| line.trim().strip_prefix("Port: "))
                    .unwrap_or("8081");
                // Чужой корпоративный прокси не считаем нашим: иначе
                // plugins_set/local_domains перезапишут его настройки.
                if server.trim() == "127.0.0.1" && port.trim() == "8081" {
                    return Ok(ProxyState {
                        enabled: true,
                        mode: Some("static".to_string()),
                        value: Some(PROXY_ADDR.to_string()),
                    });
                }
            }
        }
        Ok(ProxyState {
            enabled: false,
            mode: None,
            value: None,
        })
    }

    pub fn disable_on_exit() {
        let _ = set_system_proxy(false, &[]);
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use anyhow::Result;

    use super::ProxyState;

    pub fn set_system_proxy(_enabled: bool, _local_domains: &[String]) -> Result<ProxyState> {
        anyhow::bail!("системный прокси поддерживается на Windows и macOS")
    }

    pub fn get_system_proxy() -> Result<ProxyState> {
        Ok(ProxyState {
            enabled: false,
            mode: None,
            value: None,
        })
    }

    pub fn disable_on_exit() {}
}

pub fn set_system_proxy(enabled: bool, local_domains: &[String]) -> Result<ProxyState> {
    imp::set_system_proxy(enabled, local_domains)
}

pub fn get_system_proxy() -> Result<ProxyState> {
    imp::get_system_proxy()
}

pub fn disable_on_exit() {
    imp::disable_on_exit();
}

#[allow(dead_code)]
pub fn _unused(_dir: &Path) {}
