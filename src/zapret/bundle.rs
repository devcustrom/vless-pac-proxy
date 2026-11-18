//! Скачивание и распаковка готовых бандлов Zapret (zapret2 / Flowseal v1).
//! URL и sha256 закреплены, архивы распаковываются в `<data-dir>/zapret`.
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::state::AppState;
use crate::wlog;

use super::preset;

/// zapret2: готовый bundle из bol-van/zapret-win-bundle (winws2, Lua,
/// blockcheck2, cygwin). Коммит закреплён, чтобы содержимое не менялось.
const ZAPRET2_VERSION: &str = "6b4375fa";
const ZAPRET2_URL: &str =
    "https://github.com/bol-van/zapret-win-bundle/archive/6b4375fa04f6bd7017eec379e16e462d3ea8df66.zip";
const ZAPRET2_SHA256: &str = "c4fc3c65c802ca5dd9d3e68811a669d3d97bd5c0cd63f33afc6ae85aec2f2368";

/// Flowseal v1: general*.bat + bin/winws.exe.
const FLOWSEAL_VERSION: &str = "1.10.4";
const FLOWSEAL_URL: &str = "https://github.com/Flowseal/zapret-discord-youtube/releases/download/1.10.4/zapret-discord-youtube-1.10.4.zip";
const FLOWSEAL_SHA256: &str = "291af673bd0238506f9548e0e94ca0abf244000f4c9a06960544fb01c5f97b34";

const MAX_ARCHIVE: u64 = 80 * 1024 * 1024;
const MAX_UNPACKED: u64 = 400 * 1024 * 1024;
const MAX_ENTRIES: usize = 20_000;
const MARKER: &str = ".wpc-bundle.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BundleVariant {
    Zapret2,
    Flowseal,
}

impl BundleVariant {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "zapret2" | "zapret" | "v2" => Some(Self::Zapret2),
            "flowseal" | "v1" => Some(Self::Flowseal),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Zapret2 => "zapret2",
            Self::Flowseal => "flowseal",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Zapret2 => "zapret2",
            Self::Flowseal => "Flowseal v1",
        }
    }

    fn version(self) -> &'static str {
        match self {
            Self::Zapret2 => ZAPRET2_VERSION,
            Self::Flowseal => FLOWSEAL_VERSION,
        }
    }

    fn url(self) -> &'static str {
        match self {
            Self::Zapret2 => ZAPRET2_URL,
            Self::Flowseal => FLOWSEAL_URL,
        }
    }

    fn sha256(self) -> &'static str {
        match self {
            Self::Zapret2 => ZAPRET2_SHA256,
            Self::Flowseal => FLOWSEAL_SHA256,
        }
    }

    fn required(self) -> &'static [&'static str] {
        match self {
            Self::Zapret2 => &[
                "zapret-winws/winws2.exe",
                "zapret-winws/WinDivert.dll",
                "zapret-winws/WinDivert64.sys",
                "blockcheck/zapret2/blockcheck2.sh",
                "cygwin/bin/bash.exe",
            ],
            Self::Flowseal => &[
                "bin/winws.exe",
                "bin/WinDivert.dll",
                "bin/WinDivert64.sys",
                "general.bat",
                "lists/list-general.txt",
            ],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledBundle {
    pub variant: String,
    pub version: String,
    pub sha256: String,
    #[serde(default)]
    pub installed_at: String,
}

fn marker_path(dir: &Path) -> PathBuf {
    preset::bundle_dir(dir).join(MARKER)
}

/// Что сейчас лежит в `<data-dir>/zapret`: из маркера или по составу файлов.
pub fn installed_info(dir: &Path) -> Option<InstalledBundle> {
    let kind = preset::bundle_kind(dir)?;
    let detected = match kind {
        preset::BundleKind::V2 => BundleVariant::Zapret2,
        preset::BundleKind::V1 => BundleVariant::Flowseal,
    };
    if let Ok(text) = std::fs::read_to_string(marker_path(dir)) {
        if let Ok(info) = serde_json::from_str::<InstalledBundle>(&text) {
            if info.variant == detected.id() {
                return Some(info);
            }
        }
    }
    Some(InstalledBundle {
        variant: detected.id().to_string(),
        version: String::new(),
        sha256: String::new(),
        installed_at: String::new(),
    })
}

fn allowed_host(host: &str) -> bool {
    matches!(
        host,
        "github.com"
            | "codeload.github.com"
            | "objects.githubusercontent.com"
            | "release-assets.githubusercontent.com"
    ) || host.ends_with(".githubusercontent.com")
}

fn set_marker(bundle_dir: &Path, variant: BundleVariant) -> Result<()> {
    let info = InstalledBundle {
        variant: variant.id().to_string(),
        version: variant.version().to_string(),
        sha256: variant.sha256().to_string(),
        installed_at: chrono::Utc::now().to_rfc3339(),
    };
    let path = bundle_dir.join(MARKER);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&info)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

async fn download_archive(state: &AppState, variant: BundleVariant) -> Result<PathBuf> {
    let archive = state.dir.join(".zapret-download.zip");
    let _ = std::fs::remove_file(&archive);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .redirect(reqwest::redirect::Policy::limited(10))
        .user_agent(format!(
            "wireproxy-control/{} zapret-downloader",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .context("не удалось создать HTTP-клиент")?;
    let response = client
        .get(variant.url())
        .send()
        .await
        .with_context(|| format!("не удалось скачать {}", variant.label()))?;
    let final_url = response.url().clone();
    if final_url.scheme() != "https" {
        bail!("загрузка Zapret должна идти по https");
    }
    if let Some(host) = final_url.host_str() {
        if !allowed_host(host) {
            bail!("неожиданный хост загрузки Zapret: {host}");
        }
    } else {
        bail!("в URL загрузки Zapret нет хоста");
    }
    let response = response
        .error_for_status()
        .with_context(|| format!("сервер вернул ошибку для {}", variant.label()))?;
    let total = response.content_length().unwrap_or(0);
    if total > MAX_ARCHIVE {
        bail!(
            "архив Zapret больше лимита {} МБ",
            MAX_ARCHIVE / 1024 / 1024
        );
    }
    let mut file = tokio::fs::File::create(&archive).await?;
    let mut hasher = Sha256::new();
    let mut written = 0u64;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("обрыв загрузки Zapret")?;
        written += chunk.len() as u64;
        if written > MAX_ARCHIVE {
            let _ = tokio::fs::remove_file(&archive).await;
            bail!(
                "архив Zapret больше лимита {} МБ",
                MAX_ARCHIVE / 1024 / 1024
            );
        }
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    drop(file);
    let actual = hex::encode(hasher.finalize());
    if !actual.eq_ignore_ascii_case(variant.sha256()) {
        let _ = tokio::fs::remove_file(&archive).await;
        bail!("sha256 {} не совпал: получено {actual}", variant.label());
    }
    Ok(archive)
}

fn common_root(zip: &mut zip::ZipArchive<std::fs::File>) -> Result<Option<String>> {
    let mut root: Option<String> = None;
    for index in 0..zip.len() {
        let entry = zip.by_index(index)?;
        if entry.is_dir() {
            continue;
        }
        let Some(path) = entry.enclosed_name() else {
            continue;
        };
        let parts: Vec<_> = path.components().collect();
        if parts.len() < 2 {
            return Ok(None);
        }
        let first = parts[0].as_os_str().to_string_lossy().to_string();
        match &root {
            None => root = Some(first),
            Some(existing) if *existing != first => return Ok(None),
            _ => {}
        }
    }
    Ok(root)
}

fn unpack_zip(archive: &Path, dest: &Path, variant: BundleVariant) -> Result<()> {
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file)?;
    if zip.len() > MAX_ENTRIES {
        bail!("в архиве Zapret слишком много файлов");
    }
    let root = common_root(&mut zip)?;
    let mut unpacked = 0u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        let Some(path) = entry.enclosed_name() else {
            bail!("небезопасный путь в архиве Zapret");
        };
        let relative = match &root {
            Some(root) => {
                let mut parts = path.components();
                let first = parts
                    .next()
                    .map(|part| part.as_os_str().to_string_lossy().to_string())
                    .unwrap_or_default();
                if first != *root {
                    continue;
                }
                parts.as_path().to_path_buf()
            }
            None => path,
        };
        if relative.as_os_str().is_empty() {
            continue;
        }
        let target = dest.join(&relative);
        if !target.starts_with(dest) {
            bail!("небезопасный путь в архиве Zapret");
        }
        if entry.is_dir() {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        unpacked = unpacked.saturating_add(entry.size());
        if unpacked > MAX_UNPACKED {
            bail!(
                "распакованный Zapret больше лимита {} МБ",
                MAX_UNPACKED / 1024 / 1024
            );
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut out = std::fs::File::create(&target)?;
        std::io::copy(&mut entry, &mut out)?;
        out.sync_all()?;
    }
    for required in variant.required() {
        let path = dest.join(required);
        if !path.is_file() {
            bail!(
                "в архиве {} нет {} — возможно, изменилась структура бандла",
                variant.label(),
                required
            );
        }
    }
    set_marker(dest, variant)?;
    Ok(())
}

/// Если прошлая замена оборвалась между rename(zapret→zapret.old) и
/// rename(unpacked→zapret), возвращаем последний рабочий бандл.
pub fn recover(data_dir: &Path) {
    let target = preset::bundle_dir(data_dir);
    let backup = data_dir.join("zapret.old");
    if !target.exists() && backup.exists() {
        if let Err(e) = std::fs::rename(&backup, &target) {
            wlog!("zapret: не удалось восстановить zapret.old — {e:#}");
        }
    }
}

fn replace_bundle(data_dir: &Path, unpacked: &Path) -> Result<()> {
    recover(data_dir);
    let target = preset::bundle_dir(data_dir);
    let backup = data_dir.join("zapret.old");
    if backup.exists() {
        let _ = std::fs::remove_dir_all(&backup);
    }
    if target.exists() {
        std::fs::rename(&target, &backup).with_context(|| {
            format!(
                "не удалось заменить {} — остановите службу Zapret и повторите",
                target.display()
            )
        })?;
    }
    match std::fs::rename(unpacked, &target) {
        Ok(()) => {
            let _ = std::fs::remove_dir_all(&backup);
            Ok(())
        }
        Err(e) => {
            if backup.exists() {
                let _ = std::fs::rename(&backup, &target);
            }
            Err(e).context("не удалось установить распакованный Zapret")
        }
    }
}

/// Скачивает, проверяет и заменяет бандл. Служба на время замены
/// останавливается; после успешной замены запускается снова.
pub async fn install(state: &Arc<AppState>, variant: BundleVariant) -> Result<InstalledBundle> {
    if !cfg!(windows) {
        bail!("Zapret поддерживается только на Windows");
    }
    static INSTALL_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    let _guard = INSTALL_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    let data_dir = state.dir.clone();
    let archive = data_dir.join(".zapret-download.zip");
    let unpacked = data_dir.join(format!("zapret-unpack-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&unpacked);
    std::fs::create_dir_all(&unpacked)?;

    #[cfg(windows)]
    let mut was_running = false;
    #[cfg(windows)]
    {
        let status = super::service::query(&data_dir);
        if status.running {
            was_running = true;
            if let Err(e) = super::service::stop() {
                wlog!("zapret: остановка перед заменой бандла — {e:#}");
            }
            tokio::time::sleep(Duration::from_millis(900)).await;
        }
    }

    let result = async {
        let archive = download_archive(state, variant).await?;
        let dest = unpacked.clone();
        tokio::task::spawn_blocking(move || unpack_zip(&archive, &dest, variant))
            .await
            .context("распаковка Zapret упала")??;
        replace_bundle(&data_dir, &unpacked)?;
        if let Err(e) = preset::ensure_user_lists(&data_dir) {
            wlog!("zapret: пользовательские списки — {e:#}");
        }
        Ok::<(), anyhow::Error>(())
    }
    .await;

    if let Err(e) = result {
        let _ = std::fs::remove_dir_all(&unpacked);
        let _ = std::fs::remove_file(&archive);
        #[cfg(windows)]
        if was_running {
            let _ = super::service::start();
        }
        return Err(e);
    }

    let _ = std::fs::remove_file(&archive);
    wlog!(
        "zapret: установлен бандл {} ({})",
        variant.label(),
        variant.version()
    );
    #[cfg(windows)]
    if was_running {
        if let Err(e) = super::service::start() {
            wlog!("zapret: запуск после замены бандла — {e:#}");
            super::record_error(
                state,
                &anyhow::anyhow!(
                    "бандл заменён, но служба не запустилась: {e:#}. Нажмите «Применить пресет»"
                ),
            );
        }
    }
    installed_info(&data_dir).context("бандл распакован, но не удалось определить его состав")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_parse_and_pin_hashes() {
        assert_eq!(
            BundleVariant::parse("zapret2"),
            Some(BundleVariant::Zapret2)
        );
        assert_eq!(
            BundleVariant::parse("Flowseal"),
            Some(BundleVariant::Flowseal)
        );
        assert_eq!(BundleVariant::parse("bad"), None);
        for variant in [BundleVariant::Zapret2, BundleVariant::Flowseal] {
            assert_eq!(variant.sha256().len(), 64);
            assert!(variant.url().starts_with("https://github.com/"));
        }
    }

    #[test]
    fn creates_zip_with_common_root() {
        use std::io::Write as _;
        let dir = std::env::temp_dir().join(format!("wpc-zapret-zip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let archive = dir.join("test.zip");
        {
            let file = std::fs::File::create(&archive).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("bundle/bin/winws.exe", options).unwrap();
            zip.write_all(b"test").unwrap();
            zip.start_file("bundle/bin/WinDivert.dll", options).unwrap();
            zip.write_all(b"test").unwrap();
            zip.start_file("bundle/bin/WinDivert64.sys", options)
                .unwrap();
            zip.write_all(b"test").unwrap();
            zip.start_file("bundle/lists/list-general.txt", options)
                .unwrap();
            zip.write_all(b"test").unwrap();
            zip.start_file("bundle/general.bat", options).unwrap();
            zip.write_all(b"test").unwrap();
            zip.finish().unwrap();
        }
        let dest = dir.join("out");
        std::fs::create_dir_all(&dest).unwrap();
        unpack_zip(&archive, &dest, BundleVariant::Flowseal).unwrap();
        assert!(dest.join("bin/winws.exe").is_file());
        assert!(dest.join("general.bat").is_file());
        assert!(dest.join(MARKER).is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn temp_dir() -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "wpc-zapret-bundle-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn rejects_zip_slip() {
        use std::io::Write as _;
        let dir = temp_dir();
        let archive = dir.join("slip.zip");
        {
            let file = std::fs::File::create(&archive).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("../escape.txt", options).unwrap();
            zip.write_all(b"bad").unwrap();
            zip.finish().unwrap();
        }
        let dest = dir.join("out");
        std::fs::create_dir_all(&dest).unwrap();
        assert!(unpack_zip(&archive, &dest, BundleVariant::Flowseal).is_err());
        assert!(!dir.join("escape.txt").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn restores_backup_after_interrupted_swap() {
        let dir = temp_dir();
        let backup = dir.join("zapret.old");
        std::fs::create_dir_all(&backup).unwrap();
        std::fs::write(backup.join("winws2.exe"), b"exe").unwrap();
        recover(&dir);
        assert!(dir.join("zapret/winws2.exe").is_file());
        assert!(!backup.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
