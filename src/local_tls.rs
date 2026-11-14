//! Локальный CA и сертификаты для HTTPS-доменов плагина (rcgen + rustls).
//! Переиспользует существующий `certs/ca.key|ca.crt`, если он читается
//! (доверие пользователя сохраняется), иначе создаёт новый CA.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use rcgen::{BasicConstraints, Certificate, CertificateParams, DnType, IsCa};
use rustls::crypto::ring::sign::any_supported_type;
use rustls::pki_types::CertificateDer;
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;

#[derive(Debug, Default)]
pub struct CertStore {
    map: HashMap<String, Arc<CertifiedKey>>,
    default: Option<Arc<CertifiedKey>>,
}

impl ResolvesServerCert for CertStore {
    fn resolve(&self, hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        hello
            .server_name()
            .and_then(|n| self.map.get(n).cloned())
            .or_else(|| self.default.clone())
    }
}

fn certs_dir(dir: &Path) -> PathBuf {
    dir.join("certs")
}

fn file_in_certs(dir: &Path, name: &str) -> Result<PathBuf> {
    if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
        bail!("некорректное имя файла сертификата");
    }
    let base = certs_dir(dir);
    std::fs::create_dir_all(&base)?;
    Ok(base.join(name))
}

fn ensure_ca(dir: &Path) -> Result<Certificate> {
    let key_path = file_in_certs(dir, "ca.key")?;
    let crt_path = file_in_certs(dir, "ca.crt")?;
    if key_path.is_file() && crt_path.is_file() {
        let key_pem = std::fs::read_to_string(&key_path)?;
        let crt_pem = std::fs::read_to_string(&crt_path)?;
        let key = rcgen::KeyPair::from_pem(&key_pem)
            .context("не удалось прочитать существующий CA-ключ")?;
        let params = CertificateParams::from_ca_cert_pem(&crt_pem, key)
            .context("не удалось разобрать существующий CA")?;
        return Certificate::from_params(params).context("CA-сертификат не собран");
    }
    let mut params = CertificateParams::new(Vec::<String>::new());
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params
        .distinguished_name
        .push(DnType::CommonName, "wireproxy-control local CA");
    params.not_before = rcgen::date_time_ymd(2024, 1, 1);
    params.not_after = rcgen::date_time_ymd(2036, 1, 1);
    let ca = Certificate::from_params(params)?;
    let tmp_key = key_path.with_extension("key.tmp");
    let tmp_crt = crt_path.with_extension("crt.tmp");
    std::fs::write(&tmp_key, ca.serialize_private_key_pem())?;
    std::fs::write(&tmp_crt, ca.serialize_pem()?)?;
    std::fs::rename(tmp_key, &key_path)?;
    std::fs::rename(tmp_crt, &crt_path)?;
    Ok(ca)
}

fn domain_cert(ca: &Certificate, dir: &Path, domain: &str) -> Result<(String, String)> {
    let key_path = file_in_certs(dir, &format!("{domain}.key"))?;
    let crt_path = file_in_certs(dir, &format!("{domain}.crt"))?;
    if key_path.is_file() && crt_path.is_file() {
        let key = std::fs::read_to_string(&key_path)?;
        let crt = std::fs::read_to_string(&crt_path)?;
        if !key.trim().is_empty() && !crt.trim().is_empty() {
            return Ok((crt, key));
        }
    }
    let mut params = CertificateParams::new(vec![domain.to_string()]);
    params.distinguished_name.push(DnType::CommonName, domain);
    params.not_before = rcgen::date_time_ymd(2024, 1, 1);
    params.not_after = rcgen::date_time_ymd(2028, 1, 1);
    let leaf = Certificate::from_params(params)?;
    let cert_pem = leaf.serialize_pem_with_signer(ca)?;
    let key_pem = leaf.serialize_private_key_pem();
    let tmp_key = key_path.with_extension("key.tmp");
    let tmp_crt = crt_path.with_extension("crt.tmp");
    std::fs::write(&tmp_key, &key_pem)?;
    std::fs::write(&tmp_crt, &cert_pem)?;
    std::fs::rename(tmp_key, &key_path)?;
    std::fs::rename(tmp_crt, &crt_path)?;
    Ok((cert_pem, key_pem))
}

fn certified(cert_pem: &str, key_pem: &str) -> Result<Arc<CertifiedKey>> {
    let certs: Vec<CertificateDer<'static>> =
        rustls_pemfile::certs(&mut cert_pem.as_bytes()).collect::<std::result::Result<_, _>>()?;
    if certs.is_empty() {
        bail!("в сертификате нет цепочки");
    }
    let key =
        rustls_pemfile::private_key(&mut key_pem.as_bytes())?.context("нет приватного ключа")?;
    let signing = any_supported_type(&key)?;
    Ok(Arc::new(CertifiedKey::new(certs, signing)))
}

/// Собирает стор сертификатов для списка доменов (CA переиспользуется).
pub fn build_store(dir: &Path, domains: &[String]) -> Result<CertStore> {
    let ca = ensure_ca(dir)?;
    let mut store = CertStore::default();
    for domain in domains {
        let (cert_pem, key_pem) = domain_cert(&ca, dir, domain)?;
        let key = certified(&cert_pem, &key_pem)?;
        store.map.insert(domain.to_ascii_lowercase(), key.clone());
        if store.default.is_none() {
            store.default = Some(key);
        }
    }
    Ok(store)
}

pub fn server_config(store: Arc<CertStore>) -> Arc<rustls::ServerConfig> {
    Arc::new(
        rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_cert_resolver(store),
    )
}

pub fn ca_path(dir: &Path) -> PathBuf {
    certs_dir(dir).join("ca.crt")
}

/// Имя готового скрипта доверия CA (для панели/трея).
pub fn trust_script_name() -> &'static str {
    if cfg!(windows) {
        "local-domains-trust.bat"
    } else {
        "local-domains-trust.command"
    }
}

/// Создаёт CA, если его ещё нет (нужно до появления локальных доменов).
pub fn ensure_ca_ready(dir: &Path) -> Result<()> {
    ensure_ca(dir).map(|_| ())
}

/// Установка CA в доверенные: пишет готовый скрипт и печатает инструкцию
/// (сам бинарник права не повышает — этим занимается скрипт/трей).
pub fn trust_certs(dir: &Path) -> Result<()> {
    let ca = ca_path(dir);
    if !ca.is_file() {
        ensure_ca_ready(dir).context("не удалось создать локальный CA")?;
    }
    #[cfg(windows)]
    {
        let script = dir.join(trust_script_name());
        let content = "@echo off\r\n\
             net session >nul 2>&1\r\n\
             if errorlevel 1 (\r\n\
               echo Requesting administrator rights...\r\n\
               powershell -NoProfile -Command \"Start-Process -FilePath '%~f0' -Verb RunAs\"\r\n\
               exit /b\r\n\
             )\r\n\
             certutil -addstore -f Root \"%~dp0certs\\ca.crt\"\r\n\
             certutil -addstore -user Root \"%~dp0certs\\ca.crt\" >nul 2>&1\r\n\
             echo CA installed. Chrome/Edge now trust local HTTPS domains.\r\n\
             echo Firefox: about:config -> security.enterprise_roots.enabled = true\r\n\
             pause\r\n";
        std::fs::write(&script, content)?;
        println!(
            "Скрипт установки CA: {}\nЗапустите его (запросит UAC), затем перезапустите браузер.",
            script.display()
        );
    }
    #[cfg(not(windows))]
    {
        let script = dir.join(trust_script_name());
        let content = format!(
            "#!/bin/sh\nsudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain '{}'\necho CA installed. Firefox: security.enterprise_roots.enabled=true\n",
            ca.display()
        );
        std::fs::write(&script, content)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755));
        }
        println!(
            "Скрипт установки CA: {}\nЗапустите его (запросит пароль администратора).",
            script.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_ca_and_leaf() {
        let dir = std::env::temp_dir().join(format!("wpc-tls-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("certs")).unwrap();
        let store = build_store(&dir, &["app.localhost".into(), "panel.localhost".into()]).unwrap();
        assert!(store.map.contains_key("app.localhost"));
        assert!(store.default.is_some());
        assert!(dir.join("certs").join("ca.crt").is_file());
        assert!(dir.join("certs").join("app.localhost.crt").is_file());
        // повторная сборка переиспользует CA
        let store2 = build_store(&dir, &["app.localhost".into()]).unwrap();
        assert!(store2.map.contains_key("app.localhost"));
        let _ = server_config(Arc::new(store));
    }

    #[test]
    fn trust_without_domains_creates_ca() {
        let dir = std::env::temp_dir().join(format!("wpc-tls-empty-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("certs")).unwrap();
        assert!(!ca_path(&dir).is_file());
        trust_certs(&dir).unwrap();
        assert!(ca_path(&dir).is_file());
        assert!(dir.join("certs").join("ca.key").is_file());
        assert!(dir.join(trust_script_name()).is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
