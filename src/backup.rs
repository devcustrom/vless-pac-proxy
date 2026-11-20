//! Резервная копия данных и восстановление.
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Result};
use zip::write::SimpleFileOptions;
use zip::{ZipArchive, ZipWriter};

use crate::state::AppState;
use crate::wlog;

const ALLOWED_FILES: &[&str] = &[
    "settings.json",
    "local-domains.txt",
    "extra-domains.txt",
    "vless-domains.txt",
    "wg-domains.txt",
    "subscriptions.txt",
    "pool-state.json",
    "proxy.pac",
    "tgws-config.json",
    "wireproxy.db",
    "hosts-entries.txt",
];

fn allowed(name: &str) -> bool {
    if ALLOWED_FILES.contains(&name) {
        return true;
    }
    name.strip_prefix("certs/")
        .map(|rest| !rest.is_empty() && !rest.contains('/') && !rest.contains('\\'))
        .unwrap_or(false)
}

fn add_file(zip: &mut ZipWriter<Cursor<Vec<u8>>>, base: &Path, name: &str) -> Result<()> {
    let path = base.join(name);
    if !path.is_file() {
        return Ok(());
    }
    let mut file = std::fs::File::open(&path)?;
    let mut data = Vec::new();
    file.read_to_end(&mut data)?;
    zip.start_file(name, SimpleFileOptions::default())?;
    zip.write_all(&data)?;
    Ok(())
}

/// Собирает zip в память. Секреты (CA key, конфиги) входят в бэкап.
pub fn create(state: &Arc<AppState>) -> Result<Vec<u8>> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for name in ALLOWED_FILES
        .iter()
        .copied()
        .filter(|name| *name != "wireproxy.db")
    {
        add_file(&mut zip, &state.dir, name)?;
    }
    let certs = state.dir.join("certs");
    if let Ok(entries) = std::fs::read_dir(&certs) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                if let Some(name) = entry.file_name().to_str() {
                    add_file(&mut zip, &state.dir, &format!("certs/{name}"))?;
                }
            }
        }
    }
    let db_snapshot = state.dir.join(".backup-wireproxy.db");
    state.store.backup_to(&db_snapshot)?;
    let mut db_data = Vec::new();
    std::fs::File::open(&db_snapshot)?.read_to_end(&mut db_data)?;
    zip.start_file("wireproxy.db", SimpleFileOptions::default())?;
    zip.write_all(&db_data)?;
    let _ = std::fs::remove_file(&db_snapshot);
    let manifest = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "createdAt": chrono::Utc::now().to_rfc3339(),
        "hwid": state.settings_snapshot().hwid,
    });
    zip.start_file("backup-manifest.json", SimpleFileOptions::default())?;
    zip.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;
    Ok(zip.finish()?.into_inner())
}

fn extract(zip_path: &Path, data_dir: &Path) -> Result<()> {
    let file = std::fs::File::open(zip_path)?;
    let mut archive = ZipArchive::new(file)?;
    if archive.len() > 10_000 {
        bail!("слишком много файлов в архиве");
    }
    let mut total = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        if entry.is_dir() {
            continue;
        }
        let Some(rel) = entry.enclosed_name() else {
            bail!("небезопасный путь в архиве");
        };
        let name = rel.to_string_lossy().replace('\\', "/");
        if !allowed(&name) {
            continue;
        }
        total = total.saturating_add(entry.size());
        if total > 100 * 1024 * 1024 {
            bail!("архив больше лимита");
        }
        let target = data_dir.join(&rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = target.with_extension("restore.tmp");
        let mut out = std::fs::File::create(&tmp)?;
        std::io::copy(&mut entry, &mut out)?;
        out.flush()?;
        if target.exists() {
            std::fs::remove_file(&target)?;
        }
        std::fs::rename(&tmp, &target)?;
    }
    let _ = std::fs::remove_file(data_dir.join("wireproxy.db-wal"));
    let _ = std::fs::remove_file(data_dir.join("wireproxy.db-shm"));
    Ok(())
}

/// Вызывается после выхода менеджера: распаковать архив и снова запустить app.
pub fn apply_restore(pid: u32, zip_path: &Path, data_dir: &Path, panel_port: u16) -> Result<()> {
    crate::update::wait_for_exit(pid)?;
    let result = extract(zip_path, data_dir);
    let _ = std::fs::remove_file(zip_path);
    if let Err(e) = result {
        wlog!("восстановление бэкапа: {e:#}; запускаю прежнюю версию");
        let _ = crate::update::start_target(&std::env::current_exe()?, data_dir, panel_port);
        return Err(e);
    }
    wlog!("бэкап восстановлен, запускаю менеджер");
    crate::update::start_target(&std::env::current_exe()?, data_dir, panel_port)
}

pub fn spawn_restore(state: &Arc<AppState>, zip_path: &Path) -> Result<()> {
    let exe = std::env::current_exe()?.canonicalize()?;
    let args = vec![
        std::ffi::OsString::from("--data-dir"),
        state.dir.as_os_str().to_owned(),
        std::ffi::OsString::from("--panel-port"),
        std::ffi::OsString::from(state.panel_port.to_string()),
        std::ffi::OsString::from("restore"),
        std::ffi::OsString::from("--pid"),
        std::ffi::OsString::from(std::process::id().to_string()),
        std::ffi::OsString::from("--zip"),
        zip_path.as_os_str().to_owned(),
    ];
    crate::update::run_detached(&exe, &args)
}

pub fn restore_zip_path(data_dir: &Path) -> PathBuf {
    data_dir.join("restore-upload.zip")
}

#[cfg(test)]
mod tests {
    use super::allowed;

    #[test]
    fn only_expected_files_are_restored() {
        assert!(allowed("settings.json"));
        assert!(allowed("certs/ca.crt"));
        assert!(!allowed("../settings.json"));
        assert!(!allowed("manager.log"));
        assert!(!allowed("certs/../evil"));
        assert!(!allowed("wireproxy-control.exe"));
    }
}
