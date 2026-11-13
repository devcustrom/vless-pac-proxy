//! wireproxy: проверка wg.conf и генерация wireproxy.conf.
//!
//! `wireproxy.conf` — управляемый файл: путь `WGConfig` пересчитывается от
//! каталога данных, поэтому portable-папку можно переносить.
use std::path::Path;

use anyhow::{bail, Result};

const MAX_WG_SIZE: usize = 64 * 1024;

fn has_line(text: &str, prefix: &str) -> bool {
    text.lines()
        .any(|line| line.trim_start().to_lowercase().starts_with(prefix))
}

fn endpoint_value(text: &str) -> Option<&str> {
    text.lines()
        .find(|line| line.trim_start().to_lowercase().starts_with("endpoint"))
        .and_then(|line| line.split_once('='))
        .map(|(_, value)| value.trim())
}

fn text_configured(text: &str) -> bool {
    if text.len() > MAX_WG_SIZE {
        return false;
    }
    if !has_line(text, "[interface]")
        || !has_line(text, "[peer]")
        || !has_line(text, "privatekey")
        || !has_line(text, "publickey")
        || endpoint_value(text).is_none()
    {
        return false;
    }
    let placeholders = ["ВАШ_", "ПУБЛИЧНЫЙ_КЛЮЧ_СЕРВЕРА", "VPN.EXAMPLE.COM"];
    !placeholders.iter().any(|p| text.to_uppercase().contains(p))
}

/// Валидация присланного из панели wg.conf (без записи на диск).
pub fn validate(text: &str) -> Result<()> {
    if text.trim().is_empty() {
        bail!("пустой конфиг");
    }
    if text.len() > MAX_WG_SIZE {
        bail!("конфиг слишком большой");
    }
    if !has_line(text, "[interface]") {
        bail!("нет секции [Interface]");
    }
    if !has_line(text, "[peer]") {
        bail!("нет секции [Peer]");
    }
    if !has_line(text, "privatekey") {
        bail!("нет PrivateKey");
    }
    if !has_line(text, "publickey") {
        bail!("нет PublicKey");
    }
    let Some(endpoint) = endpoint_value(text) else {
        bail!("нет Endpoint");
    };
    let Some((_, port)) = endpoint.rsplit_once(':') else {
        bail!("Endpoint должен быть host:port");
    };
    let port: u16 = port
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("порт в Endpoint"))?;
    if port == 0 {
        bail!("порт в Endpoint");
    }
    let placeholders = ["ВАШ_", "ПУБЛИЧНЫЙ_КЛЮЧ_СЕРВЕРА", "VPN.EXAMPLE.COM"];
    if placeholders.iter().any(|p| text.to_uppercase().contains(p)) {
        bail!("в конфиге остались заглушки из примера");
    }
    Ok(())
}

/// «wg.conf настроен»: есть ключи, обе секции и нет заглушек.
pub fn is_configured(dir: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(dir.join("wg.conf")) else {
        return false;
    };
    text_configured(&text)
}

fn desired_config(dir: &Path) -> Result<String> {
    let base = std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf());
    Ok(format!(
        "# wireproxy: WireGuard-userspace -> SOCKS5/HTTP proxy\n\
         # Сгенерировано wireproxy-control (файл управляется автоматически).\n\
         WGConfig = {}\n\n\
         [Socks5]\n\
         BindAddress = 127.0.0.1:{}\n\n\
         [http]\n\
         BindAddress = 127.0.0.1:{}\n",
        base.join("wg.conf").display(),
        crate::vless::WIREPROXY_SOCKS_PORT,
        crate::vless::WIREPROXY_HTTP_PORT
    ))
}

/// Генерирует/обновляет wireproxy.conf под текущий каталог данных.
/// Возвращает `true`, если файл изменился.
pub fn ensure_config(dir: &Path) -> Result<bool> {
    if !is_configured(dir) {
        return Ok(false);
    }
    let desired = desired_config(dir)?;
    let path = dir.join("wireproxy.conf");
    if let Ok(current) = std::fs::read_to_string(&path) {
        if current == desired {
            return Ok(false);
        }
    }
    let tmp = dir.join("wireproxy.conf.tmp");
    std::fs::write(&tmp, desired)?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    std::fs::rename(&tmp, &path)?;
    Ok(true)
}

/// Проверка перед запуском движка: пересобирает конфиг при необходимости.
pub fn prepare(dir: &Path) -> bool {
    if !is_configured(dir) {
        return false;
    }
    match ensure_config(dir) {
        Ok(true) => {
            crate::wlog!("wireproxy.conf: обновлён путь WGConfig");
            true
        }
        Ok(false) => true,
        Err(e) => {
            crate::wlog!("wireproxy.conf: не удалось обновить — {e:#}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "[Interface]\nPrivateKey = abc\nAddress = 10.0.0.2/32\n\
                        [Peer]\nPublicKey = def\nEndpoint = 1.2.3.4:51820\n";

    fn tmpdir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("wpc-test-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn validates_good_and_rejects_bad() {
        assert!(validate(GOOD).is_ok());
        assert!(validate("").is_err());
        assert!(validate("[Interface]\nPrivateKey = a\n").is_err());
        assert!(validate(&GOOD.replace("[Peer]", "")).is_err());
        assert!(validate(&GOOD.replace("1.2.3.4:51820", "1.2.3.4")).is_err());
        assert!(validate(&GOOD.replace("abc", "ВАШ_ПРИВАТНЫЙ_КЛЮЧ")).is_err());
    }

    #[test]
    fn detects_placeholders() {
        let d = tmpdir("placeholder");
        std::fs::write(
            d.join("wg.conf"),
            "[Interface]\nPrivateKey = ВАШ_ПРИВАТНЫЙ_КЛЮЧ\n\
             [Peer]\nPublicKey = x\nEndpoint = vpn.example.com:51820",
        )
        .unwrap();
        assert!(!is_configured(&d));
        std::fs::write(d.join("wg.conf"), GOOD).unwrap();
        assert!(is_configured(&d));
    }

    #[test]
    fn generates_and_updates_config() {
        let d = tmpdir("conf");
        std::fs::write(d.join("wg.conf"), GOOD).unwrap();
        assert!(ensure_config(&d).unwrap());
        let text = std::fs::read_to_string(d.join("wireproxy.conf")).unwrap();
        assert!(text.contains("WGConfig ="));
        assert!(text.contains("wg.conf"));
        assert!(text.contains("127.0.0.1:25344"));
        assert!(text.contains("127.0.0.1:25345"));
        assert!(!ensure_config(&d).unwrap());
        // старый путь (перенос папки) пересчитывается
        std::fs::write(d.join("wireproxy.conf"), "WGConfig = C:\\old\\wg.conf\n").unwrap();
        assert!(ensure_config(&d).unwrap());
        assert!(prepare(&d));
        let _ = std::fs::remove_dir_all(&d);
    }
}
