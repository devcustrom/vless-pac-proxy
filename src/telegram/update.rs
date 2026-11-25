use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use url::Url;

use crate::state::AppState;
use crate::wlog;

use super::supervisor;
use super::UpdateState;

const MANIFEST_URL: &str =
    "https://github.com/devcustrom/vless-pac-proxy/releases/latest/download/tgws-manifest.json";
const VERSION_FILE: &str = "tgws-version.json";
const MAX_MANIFEST: usize = 1024 * 1024;
const MAX_ASSET: u64 = 80 * 1024 * 1024;
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Debug, Clone, Deserialize)]
struct Manifest {
    version: String,
    #[serde(default)]
    assets: BTreeMap<String, Asset>,
}

#[derive(Debug, Clone, Deserialize)]
struct Asset {
    url: String,
    sha256: String,
    #[serde(default)]
    size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Installed {
    version: String,
    sha256: String,
}

struct Guard<'a>(&'a AtomicBool);

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

pub fn platform_key() -> &'static str {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    match (os, arch) {
        ("windows", "x86_64") => "windows-x64",
        ("macos", "x86_64") => "macos-x64",
        ("macos", "aarch64") => "macos-arm64",
        _ => "unsupported",
    }
}

fn installed_path(dir: &Path) -> PathBuf {
    dir.join(VERSION_FILE)
}

fn read_installed(dir: &Path) -> Option<Installed> {
    let text = std::fs::read_to_string(installed_path(dir)).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_installed(dir: &Path, version: &str, sha256: &str) -> Result<()> {
    let path = installed_path(dir);
    let tmp = path.with_extension("json.tmp");
    let data = Installed {
        version: version.to_string(),
        sha256: sha256.to_string(),
    };
    std::fs::write(&tmp, serde_json::to_string_pretty(&data)?)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

fn local_manifest_path() -> Option<PathBuf> {
    std::env::var_os("WPC_TGWS_LOCAL_MANIFEST").map(PathBuf::from)
}

async fn load_manifest() -> Result<Manifest> {
    let text = if let Some(path) = local_manifest_path() {
        std::fs::read_to_string(&path)
            .with_context(|| format!("не удалось прочитать манифест {}", path.display()))?
    } else {
        let client = http_client()?;
        let response = open_url(&client, MANIFEST_URL, 6).await?;
        let bytes = read_limited(response, MAX_MANIFEST).await?;
        String::from_utf8(bytes).context("манифест не UTF-8")?
    };
    let manifest: Manifest = serde_json::from_str(&text).context("некорректный манифест")?;
    if manifest.version.trim().is_empty() {
        bail!("в манифесте нет версии");
    }
    Ok(manifest)
}

fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(60))
        .user_agent(format!(
            "wireproxy-control/{} tgws-updater",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .context("не удалось создать HTTP-клиент")
}

async fn open_url(
    client: &reqwest::Client,
    raw: &str,
    max_redirects: usize,
) -> Result<reqwest::Response> {
    let mut url = Url::parse(raw).context("некорректный URL обновления")?;
    for _ in 0..=max_redirects {
        if url.scheme() != "https" {
            bail!("разрешены только https-загрузки");
        }
        let host = url.host_str().context("в URL нет хоста")?;
        crate::net::dns_guard::check_host(host).await?;
        let response = client.get(url.clone()).send().await?;
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get(reqwest::header::LOCATION)
                .and_then(|v| v.to_str().ok())
                .context("редирект без Location")?;
            url = url.join(location).context("некорректный redirect")?;
            continue;
        }
        if !response.status().is_success() {
            bail!("HTTP {} при загрузке обновления", response.status());
        }
        return Ok(response);
    }
    bail!("слишком много редиректов")
}

async fn read_limited(response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    let mut data = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if data.len() + chunk.len() > limit {
            bail!("ответ больше лимита {limit} байт");
        }
        data.extend_from_slice(&chunk);
    }
    Ok(data)
}

async fn download_asset(asset: &Asset, dest: &Path) -> Result<String> {
    if asset.sha256.len() != 64 || !asset.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("некорректный sha256 в манифесте");
    }
    let _ = std::fs::remove_file(dest);
    let mut hasher = Sha256::new();
    let mut written = 0u64;

    if asset.url.starts_with("http://") || asset.url.starts_with("https://") {
        let client = http_client()?;
        let response = open_url(&client, &asset.url, 6).await?;
        if let Some(expected) = asset.size {
            if response.content_length().unwrap_or(0) > expected.saturating_add(1024) {
                bail!("размер загрузки больше заявленного");
            }
        }
        let mut file = tokio::fs::File::create(dest).await?;
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            written += chunk.len() as u64;
            if written > MAX_ASSET {
                bail!("sidecar больше лимита {MAX_ASSET} байт");
            }
            hasher.update(&chunk);
            file.write_all(&chunk).await?;
        }
        file.flush().await?;
    } else {
        // Локальный манифест для dev-проверки: путь к уже собранному бинарнику.
        let src = PathBuf::from(&asset.url);
        let meta = std::fs::metadata(&src)
            .with_context(|| format!("локальный sidecar не найден: {}", src.display()))?;
        if meta.len() > MAX_ASSET {
            bail!("sidecar больше лимита {MAX_ASSET} байт");
        }
        written = meta.len();
        let data = std::fs::read(&src)?;
        hasher.update(&data);
        tokio::fs::write(dest, &data).await?;
    }

    if let Some(expected) = asset.size {
        if written != expected {
            bail!("размер sidecar {written} не совпадает с манифестом {expected}");
        }
    }
    let actual = hex::encode(hasher.finalize());
    if !actual.eq_ignore_ascii_case(&asset.sha256) {
        bail!("sha256 sidecar не совпадает");
    }
    Ok(actual)
}

async fn apply_update(state: &Arc<AppState>, manifest: &Manifest, asset: &Asset) -> Result<()> {
    let target = supervisor::sidecar_path(&state.dir);
    let temp = state.dir.join("tgws-proxy.download");
    let hash = download_asset(asset, &temp).await?;

    let was_enabled = state.settings_snapshot().telegram_enabled;
    supervisor::stop(state);
    tokio::time::sleep(Duration::from_millis(800)).await;

    let backup = state
        .dir
        .join(format!("{}.bak", supervisor::sidecar_name()));
    let _ = std::fs::remove_file(&backup);
    if target.is_file() {
        std::fs::rename(&target, &backup).context("не удалось сохранить старый sidecar")?;
    }
    let installed = (|| -> Result<()> {
        std::fs::rename(&temp, &target).context("не удалось установить sidecar")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755));
        }
        write_installed(&state.dir, &manifest.version, &hash)?;
        Ok(())
    })();
    if let Err(e) = &installed {
        let _ = std::fs::remove_file(&target);
        if backup.is_file() {
            let _ = std::fs::rename(&backup, &target);
        }
        if was_enabled {
            let _ = supervisor::start(state);
        }
        return Err(anyhow::anyhow!("{e:#}"));
    }
    let _ = std::fs::remove_file(&backup);
    wlog!("telegram: sidecar обновлён до {}", manifest.version);
    if was_enabled {
        if let Err(e) = supervisor::start(state) {
            wlog!("telegram: запуск после обновления — {e:#}");
        }
    }
    Ok(())
}

fn set_update_state(state: &AppState, f: impl FnOnce(&mut UpdateState)) {
    if let Ok(mut guard) = state.telegram.update.lock() {
        f(&mut guard);
    }
}

/// Числовая часть версии: `1.11.4+managed.5` → [1, 11, 4, 5].
fn version_key(value: &str) -> Vec<u64> {
    value
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|part| part.parse().ok())
        .collect()
}

fn installed_is_newer(installed: &str, manifest: &str) -> bool {
    version_key(installed) > version_key(manifest)
}

/// Проверяет манифест и при необходимости обновляет sidecar.
pub async fn check_and_update(state: &Arc<AppState>, _manual: bool) -> Result<String> {
    if state.telegram.updating.swap(true, Ordering::SeqCst) {
        bail!("обновление уже выполняется");
    }
    let _guard = Guard(&state.telegram.updating);
    let at = chrono::Utc::now().to_rfc3339();
    set_update_state(state, |s| {
        s.checking = true;
        s.last_check = Some(at.clone());
        s.last_error = None;
    });

    let result = async {
        let manifest = load_manifest().await?;
        let asset = manifest
            .assets
            .get(platform_key())
            .with_context(|| format!("в манифесте нет сборки для {}", platform_key()))?;
        let target = supervisor::sidecar_path(&state.dir);
        let installed = read_installed(&state.dir);
        if let Some(installed) = &installed {
            let up_to_date = installed.version == manifest.version
                && installed.sha256.eq_ignore_ascii_case(&asset.sha256)
                && target.is_file();
            // Локальная сборка может быть новее манифеста (например, до
            // публикации релиза). Не откатываем её на старую версию.
            let local_newer =
                installed_is_newer(&installed.version, &manifest.version) && target.is_file();
            if up_to_date || local_newer {
                set_update_state(state, |s| {
                    s.installed_version = Some(installed.version.clone());
                    s.available_version = Some(installed.version.clone());
                });
                return Ok(installed.version.clone());
            }
        }
        apply_update(state, &manifest, asset).await?;
        set_update_state(state, |s| {
            s.installed_version = Some(manifest.version.clone());
            s.available_version = Some(manifest.version.clone());
        });
        Ok(manifest.version)
    }
    .await;

    set_update_state(state, |s| {
        s.checking = false;
        if let Err(e) = &result {
            s.last_error = Some(format!("{e:#}"));
        }
    });
    result
}

pub fn installed_version(dir: &Path) -> Option<String> {
    read_installed(dir).map(|v| v.version)
}

pub fn clear_update_error(state: &AppState) {
    set_update_state(state, |s| s.last_error = None);
}

pub fn spawn_scheduler(state: Arc<AppState>) {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(8)).await;
        loop {
            let settings = state.settings_snapshot();
            if settings.telegram_enabled && settings.telegram_auto_update {
                if let Err(e) = check_and_update(&state, false).await {
                    let text = format!("{e:#}");
                    if text.contains("HTTP 404") {
                        // Манифест ещё не опубликован: это не ошибка установки.
                        set_update_state(&state, |s| s.last_error = None);
                    } else {
                        wlog!("telegram: автообновление — {text}");
                    }
                }
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("wpc-tgws-{}-{stamp}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn local_asset_hash_is_verified() {
        let dir = temp_dir();
        let src = dir.join("source.exe");
        std::fs::write(&src, b"sidecar-binary").unwrap();
        let hash = hex::encode(Sha256::digest(b"sidecar-binary"));
        let asset = Asset {
            url: src.display().to_string(),
            sha256: hash,
            size: Some(b"sidecar-binary".len() as u64),
        };
        let dest = dir.join("target.exe");
        let actual = download_asset(&asset, &dest).await.unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"sidecar-binary");
        assert_eq!(actual, asset.sha256);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn bad_hash_is_rejected() {
        let dir = temp_dir();
        let src = dir.join("source.exe");
        std::fs::write(&src, b"sidecar-binary").unwrap();
        let asset = Asset {
            url: src.display().to_string(),
            sha256: "0".repeat(64),
            size: None,
        };
        let dest = dir.join("target.exe");
        assert!(download_asset(&asset, &dest).await.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn managed_build_versions_compare() {
        assert!(installed_is_newer("1.11.4+managed.5", "1.11.4+managed.4"));
        assert!(!installed_is_newer("1.11.4+managed.4", "1.11.4+managed.5"));
        assert!(!installed_is_newer("1.11.4+managed.4", "1.11.4+managed.4"));
        assert!(installed_is_newer("1.11.5", "1.11.4+managed.9"));
    }

    #[test]
    fn platform_is_known() {
        assert!(matches!(
            platform_key(),
            "windows-x64" | "macos-x64" | "macos-arm64" | "unsupported"
        ));
    }
}
