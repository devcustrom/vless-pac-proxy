//! Платформенные реализации системного прокси.
use std::path::Path;

use anyhow::Result;

pub const PROXY_ADDR: &str = "127.0.0.1:8081";
pub const PROXY_OVERRIDE: &str =
    "localhost;127.0.0.1;*.local;192.168.*;10.*;172.16.*;<local>;zcode.z.ai;*.z.ai";

#[derive(Debug, Clone, serde::Serialize)]
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

    use super::{ProxyState, PROXY_ADDR, PROXY_OVERRIDE};

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

    pub fn set_system_proxy(enabled: bool) -> Result<ProxyState> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = hkcu.open_subkey_with_flags(KEY, KEY_ALL_ACCESS)?;
        if enabled {
            key.set_value("ProxyEnable", &1u32)?;
            key.set_value("ProxyServer", &PROXY_ADDR)?;
            key.set_value("ProxyOverride", &PROXY_OVERRIDE)?;
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

#[cfg(not(windows))]
mod imp {
    use anyhow::Result;

    use super::ProxyState;

    pub fn set_system_proxy(_enabled: bool) -> Result<ProxyState> {
        anyhow::bail!("macOS-реализация системного прокси — этап M1")
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

pub fn set_system_proxy(enabled: bool) -> Result<ProxyState> {
    imp::set_system_proxy(enabled)
}

pub fn get_system_proxy() -> Result<ProxyState> {
    imp::get_system_proxy()
}

pub fn disable_on_exit() {
    imp::disable_on_exit();
}

#[allow(dead_code)]
pub fn _unused(_dir: &Path) {}
