//! Regenerates files consumed by external engine processes from SQLite.
use std::path::Path;

use anyhow::Result;

use crate::net::local_proxy::LocalDomain;
use crate::store::Store;

/// Reconciles engine inputs after a restore or a schema migration.
pub fn reconcile(store: &Store, dir: &Path, local_domains: &[LocalDomain]) -> Result<()> {
    crate::engines::wg_configs::materialize_active(store, dir)?;
    crate::vless::configs::materialize_active(store, dir)?;

    let pac = crate::paths::in_dir(dir, "proxy.pac")?;
    let pac_text = crate::lists::pac_content();
    if std::fs::read_to_string(&pac).ok().as_deref() != Some(pac_text.as_str()) {
        let tmp = pac.with_extension("tmp");
        std::fs::write(&tmp, pac_text)?;
        std::fs::rename(tmp, pac)?;
    }

    let names: Vec<String> = local_domains
        .iter()
        .map(|domain| domain.domain.clone())
        .collect();
    if let Err(error) = crate::local_hosts::write_script(dir, &names) {
        crate::wlog!("materialize: hosts-скрипты — {error:#}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_active_engine_files() {
        let dir = std::env::temp_dir().join(format!(
            "wpc-materialize-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir).unwrap();
        let wg = "[Interface]\nPrivateKey = abc\nAddress = 10.0.0.2/32\n\
                  [Peer]\nPublicKey = def\nEndpoint = 1.2.3.4:51820\n";
        store.insert_wg_profile("default", "Default", wg).unwrap();
        let xray = r#"{"outbounds":[{"protocol":"vless","settings":{"vnext":[{"address":"example.com","port":443}]}}]}"#;
        store
            .insert_xray_profile("vless", "VLESS", xray, "vless", "example.com", 443, false)
            .unwrap();
        let local = vec![LocalDomain {
            domain: "panel.localhost".into(),
            port: 8080,
        }];
        reconcile(&store, &dir, &local).unwrap();
        assert!(dir.join("wg.conf").is_file());
        assert!(dir.join("wireproxy.conf").is_file());
        assert!(dir.join("xray-config.json").is_file());
        assert!(dir.join("hosts-entries.txt").is_file());
        let first = std::fs::read_to_string(dir.join("xray-config.json")).unwrap();
        reconcile(&store, &dir, &local).unwrap();
        assert_eq!(
            first,
            std::fs::read_to_string(dir.join("xray-config.json")).unwrap()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
