//! wireproxy: spawn/monitor (M1).-пока каркас.
#![allow(dead_code)]
use std::path::Path;

/// «wg.conf настроен»: есть ключи и нет заглушек из примера (паритет с legacy).
pub fn is_configured(dir: &Path) -> bool {
    let path = dir.join("wg.conf");
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let has_key = text
        .lines()
        .any(|l| l.trim_start().to_lowercase().starts_with("privatekey"));
    let has_endpoint = text
        .lines()
        .any(|l| l.trim_start().to_lowercase().starts_with("endpoint"));
    let placeholder = ["ВАШ_", "ПУБЛИЧНЫЙ_КЛЮЧ_СЕРВЕРА", "VPN.EXAMPLE.COM"]
        .iter()
        .any(|p| text.to_uppercase().contains(p));
    has_key && has_endpoint && !placeholder
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmpdir() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("wpc-test-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn detects_placeholders() {
        let d = tmpdir();
        let mut f = std::fs::File::create(d.join("wg.conf")).unwrap();
        writeln!(
            f,
            "[Interface]\nPrivateKey = ВАШ_ПРИВАТНЫЙ_КЛЮЧ\nEndpoint = vpn.example.com:51820"
        )
        .unwrap();
        assert!(!is_configured(&d));
        std::fs::write(
            d.join("wg.conf"),
            "[Interface]\nPrivateKey = abc\n[Peer]\nEndpoint = 1.2.3.4:51820\n",
        )
        .unwrap();
        assert!(is_configured(&d));
    }
}
