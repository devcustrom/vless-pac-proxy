use anyhow::{bail, Result};

use super::config::valid_secret;

pub fn build_link(host: &str, port: u16, secret: &str, fake_tls_domain: &str) -> Result<String> {
    if !valid_secret(secret) {
        bail!("секрет должен быть 32 hex-символа");
    }
    if host.trim().is_empty() || port == 0 {
        bail!("некорректный адрес Telegram-прокси");
    }
    let prefix = if fake_tls_domain.trim().is_empty() {
        "dd".to_string()
    } else {
        format!(
            "ee{}{}",
            secret,
            hex::encode(fake_tls_domain.trim().to_ascii_lowercase())
        )
    };
    let secret_part = if prefix.starts_with("ee") {
        prefix
    } else {
        format!("dd{secret}")
    };
    Ok(format!(
        "tg://proxy?server={host}&port={port}&secret={secret_part}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn plain_link() {
        let link = build_link("127.0.0.1", 1443, SECRET, "").unwrap();
        assert_eq!(
            link,
            "tg://proxy?server=127.0.0.1&port=1443&secret=dd0123456789abcdef0123456789abcdef"
        );
    }

    #[test]
    fn fake_tls_link() {
        let link = build_link("127.0.0.1", 1443, SECRET, "example.com").unwrap();
        assert!(link.contains("secret=ee"));
        assert!(link.ends_with(&hex::encode("example.com")));
    }

    #[test]
    fn rejects_bad_secret() {
        assert!(build_link("127.0.0.1", 1443, "short", "").is_err());
    }
}
