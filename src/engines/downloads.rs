//! Загрузка движков при первом запуске: пин версий, sha256, распаковка
//! (zip/tar.gz). Прогресс виден в /api/status.engineDownloads и в панели.
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::engines::{find_engine, EngineKind};
use crate::state::AppState;
use crate::wlog;

const WIREPROXY_VERSION: &str = "v1.1.3";
const XRAY_VERSION: &str = "v26.11.9";
const WIREPROXY_BASE: &str = "https://github.com/pufferffish/wireproxy/releases/download";
const XRAY_BASE: &str = "https://github.com/XTLS/Xray-core/releases/download";

#[derive(Clone, Copy)]
enum Archive {
    TarGz,
    Zip,
}

#[derive(Clone)]
struct Asset {
    url: String,
    sha256: &'static str,
    archive: Archive,
    exe: &'static str,
    extras: &'static [&'static str],
}

fn asset(kind: EngineKind) -> Result<Asset> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let build = |url: String, sha256: &'static str, archive, exe, extras| Asset {
        url,
        sha256,
        archive,
        exe,
        extras,
    };
    match (kind, os, arch) {
        (EngineKind::Wireproxy, "windows", "x86_64") => Ok(build(
            format!("{WIREPROXY_BASE}/{WIREPROXY_VERSION}/wireproxy_windows_amd64.tar.gz"),
            "bce041ea9fe0f8a3351301dcbe29cdf6a523bb25cf9c62f17ebb5699a8051d0f",
            Archive::TarGz,
            "wireproxy.exe",
            &[],
        )),
        (EngineKind::Wireproxy, "macos", "x86_64") => Ok(build(
            format!("{WIREPROXY_BASE}/{WIREPROXY_VERSION}/wireproxy_darwin_amd64.tar.gz"),
            "5d89742a0f381d9508d3ad828a0d300dceee24fa49015427eaff42a23f7a50bd",
            Archive::TarGz,
            "wireproxy",
            &[],
        )),
        (EngineKind::Wireproxy, "macos", "aarch64") => Ok(build(
            format!("{WIREPROXY_BASE}/{WIREPROXY_VERSION}/wireproxy_darwin_arm64.tar.gz"),
            "28d34342c48c309b628d9c06ab4efc05b82fba49821f360605c02139c02547a5",
            Archive::TarGz,
            "wireproxy",
            &[],
        )),
        (EngineKind::Xray, "windows", "x86_64") => Ok(build(
            format!("{XRAY_BASE}/{XRAY_VERSION}/Xray-windows-64.zip"),
            "1262522266337b824f07beb8e1375786b437683b3890105469088f8e17e0a69d",
            Archive::Zip,
            "xray.exe",
            &["geoip.dat", "geosite.dat", "LICENSE"],
        )),
        (EngineKind::Xray, "macos", "x86_64") => Ok(build(
            format!("{XRAY_BASE}/{XRAY_VERSION}/Xray-macos-64.zip"),
            "488963e676e06f5e48547f9dd38116aaca8de4234959b4272ae8f284e658dfd0",
            Archive::Zip,
            "xray",
            &["geoip.dat", "geosite.dat", "LICENSE"],
        )),
        (EngineKind::Xray, "macos", "aarch64") => Ok(build(
            format!("{XRAY_BASE}/{XRAY_VERSION}/Xray-macos-arm64-v8a.zip"),
            "233f0c66681cd0726f81a6b2b6ed6a5710869e312047b522e46ed53035bebd26",
            Archive::Zip,
            "xray",
            &["geoip.dat", "geosite.dat", "LICENSE"],
        )),
        _ => bail!(
            "нет сборки {} для {os}/{arch} — поддерживаются Windows x64 и macOS x64/arm64",
            kind.label()
        ),
    }
}

fn set_progress(
    state: &Option<Arc<AppState>>,
    key: &str,
    stage: &str,
    done: u64,
    total: u64,
    error: Option<String>,
) {
    if let Some(st) = state {
        if let Ok(mut map) = st.downloads.lock() {
            map.insert(
                key.to_string(),
                crate::state::DownloadInfo {
                    stage: stage.to_string(),
                    done,
                    total,
                    error,
                },
            );
        }
    }
}

async fn download_once(
    dir: &Path,
    kind: EngineKind,
    asset: &Asset,
    state: &Option<Arc<AppState>>,
) -> Result<()> {
    let key = kind.label();
    set_progress(state, key, "download", 0, 0, None);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .user_agent(format!("wireproxy-control/{}", env!("CARGO_PKG_VERSION")))
        .build()?;
    let response = client
        .get(&asset.url)
        .send()
        .await
        .with_context(|| format!("не удалось скачать {}", asset.url))?
        .error_for_status()
        .with_context(|| format!("сервер вернул ошибку для {}", asset.url))?;
    let total = response.content_length().unwrap_or(0);
    let archive_path = dir.join(format!(".{key}.download"));
    let mut file = tokio::fs::File::create(&archive_path).await?;
    let mut hasher = Sha256::new();
    let mut done: u64 = 0;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("обрыв загрузки")?;
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
        done += chunk.len() as u64;
        set_progress(state, key, "download", done, total, None);
    }
    file.flush().await?;
    drop(file);

    let got = hex::encode(hasher.finalize());
    if !got.eq_ignore_ascii_case(asset.sha256) {
        let _ = tokio::fs::remove_file(&archive_path).await;
        bail!("sha256 не совпал: получено {got}");
    }

    set_progress(state, key, "unpack", done, total, None);
    let archive = archive_path.clone();
    let target_dir = dir.to_path_buf();
    let asset = asset.clone();
    tokio::task::spawn_blocking(move || unpack(&archive, &target_dir, &asset)).await??;
    let _ = tokio::fs::remove_file(&archive_path).await;
    set_progress(state, key, "done", done, total, None);
    Ok(())
}

fn unpack(archive: &Path, dir: &Path, asset: &Asset) -> Result<()> {
    let wanted: Vec<&str> = std::iter::once(asset.exe)
        .chain(asset.extras.iter().copied())
        .collect();
    match asset.archive {
        Archive::Zip => {
            let file = std::fs::File::open(archive)?;
            let mut zip = zip::ZipArchive::new(file)?;
            for name in &wanted {
                if let Ok(mut entry) = zip.by_name(name) {
                    write_member(&dir.join(name), &mut entry)?;
                }
            }
        }
        Archive::TarGz => {
            let file = std::fs::File::open(archive)?;
            let decoder = flate2::read::GzDecoder::new(file);
            let mut tar = tar::Archive::new(decoder);
            for entry in tar.entries()? {
                let mut entry = entry?;
                let path = entry.path()?.to_path_buf();
                let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                    continue;
                };
                if wanted.iter().any(|w| w.eq_ignore_ascii_case(name)) {
                    write_member(&dir.join(name), &mut entry)?;
                }
            }
        }
    }
    let exe = dir.join(asset.exe);
    if !exe.is_file() {
        bail!("в архиве не найден {}", asset.exe);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755));
    }
    wlog!("движок {}: распакован", asset.exe);
    Ok(())
}

fn write_member(target: &Path, reader: &mut impl std::io::Read) -> Result<()> {
    let tmp = target.with_extension("tmp");
    let mut out = std::fs::File::create(&tmp)?;
    std::io::copy(reader, &mut out)?;
    out.flush()?;
    if target.exists() {
        std::fs::remove_file(target)?;
    }
    std::fs::rename(&tmp, target)?;
    Ok(())
}

/// Гарантирует наличие движка в каталоге данных (скачивает при отсутствии).
pub async fn ensure(dir: &Path, kind: EngineKind, state: Option<Arc<AppState>>) -> Result<PathBuf> {
    if let Some(found) = find_engine(kind) {
        return Ok(found);
    }
    let dest = dir.join(kind.exe_name());
    if dest.is_file() {
        return Ok(dest);
    }
    let asset = asset(kind)?;
    let mut last: Option<anyhow::Error> = None;
    for attempt in 1..=2u8 {
        match download_once(dir, kind, &asset, &state).await {
            Ok(()) => {
                wlog!("движок {}: готов ({})", kind.label(), kind.exe_name());
                return Ok(dest);
            }
            Err(e) => {
                wlog!(
                    "движок {}: попытка {attempt} не удалась — {e:#}",
                    kind.label()
                );
                last = Some(e);
                if attempt < 2 {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }
    let err = last.unwrap_or_else(|| anyhow::anyhow!("не удалось скачать движок"));
    set_progress(
        &state,
        kind.label(),
        "error",
        0,
        0,
        Some(format!("{err:#}")),
    );
    Err(err)
}
