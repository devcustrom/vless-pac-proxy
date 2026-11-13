//! Проверка, что локальный CA уже в доверенных (для меню трея и панели).
use std::path::Path;

/// Windows: по SHA-1 отпечатку сертификата ищем его в Root-хранилищах
/// пользователя и машины (реестр, без WinAPI). macOS: `security
/// find-certificate` в системном keychain (аргументы фиксированные).
pub fn is_trusted(dir: &Path) -> bool {
    let ca = crate::local_tls::ca_path(dir);
    if !ca.is_file() {
        return false;
    }
    #[cfg(windows)]
    {
        let Ok(pem) = std::fs::read_to_string(&ca) else {
            return false;
        };
        let Ok(certs) = rustls_pemfile::certs(&mut pem.as_bytes()).collect::<Result<Vec<_>, _>>()
        else {
            return false;
        };
        let Some(der) = certs.first() else {
            return false;
        };
        use sha1::{Digest, Sha1};
        let mut hasher = Sha1::new();
        hasher.update(der.as_ref());
        let thumb = hex::encode_upper(hasher.finalize());
        let subkey = format!(r"Software\Microsoft\SystemCertificates\Root\Certificates\{thumb}");
        let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
        if hkcu.open_subkey(&subkey).is_ok() {
            return true;
        }
        let hklm = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE);
        hklm.open_subkey(&subkey).is_ok()
    }
    #[cfg(target_os = "macos")]
    {
        let _ = ca;
        std::process::Command::new("security")
            .args([
                "find-certificate",
                "-a",
                "-c",
                "wireproxy-control local CA",
                "/Library/Keychains/System.keychain",
            ])
            .output()
            .map(|out| out.status.success() && !out.stdout.is_empty())
            .unwrap_or(false)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = ca;
        false
    }
}
