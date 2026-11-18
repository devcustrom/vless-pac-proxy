//! Обновление самого менеджера: скачивание нового бинарника и его замена
//! отдельным процессом после выхода текущего (без ручных действий).
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::state::AppState;
use crate::wlog;

const RELEASE_API: &str = "https://api.github.com/repos/devcustrom/vless-pac-proxy/releases/latest";
const MAX_ASSET: u64 = 200 * 1024 * 1024;
static INSTALLING: AtomicBool = AtomicBool::new(false);

pub fn begin_install() -> bool {
    INSTALLING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
}

pub fn end_install() {
    INSTALLING.store(false, Ordering::SeqCst);
}

#[derive(Debug, Clone)]
pub struct ReleaseInfo {
    pub version: String,
    pub url: String,
    pub name: String,
    pub size: u64,
    pub digest: Option<String>,
    pub published_at: Option<String>,
    pub page: Option<String>,
}

impl ReleaseInfo {
    pub fn available(&self) -> bool {
        version_tuple(&self.version) > version_tuple(&current_version())
    }
}

fn current_version() -> String {
    if std::env::var_os("WPC_TEST_MODE").is_some() {
        if let Ok(value) = std::env::var("WPC_TEST_CURRENT_VERSION") {
            return value;
        }
    }
    env!("CARGO_PKG_VERSION").to_string()
}

fn version_tuple(value: &str) -> (u64, u64, u64) {
    let mut parts = value.trim_start_matches('v').split('.').map(|part| {
        part.chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>()
            .parse::<u64>()
            .unwrap_or(0)
    });
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}

fn asset_name() -> Result<&'static str> {
    if cfg!(windows) {
        Ok("wireproxy-control-windows-x64.exe")
    } else if cfg!(target_os = "macos") {
        Ok("wireproxy-control-macos-universal")
    } else {
        bail!("автообновление поддерживается только на Windows и macOS")
    }
}

pub async fn latest() -> Result<ReleaseInfo> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(format!("wireproxy-control/{}", env!("CARGO_PKG_VERSION")))
        .build()?;
    let response = client
        .get(RELEASE_API)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("GitHub недоступен")?;
    if !response.status().is_success() {
        bail!("GitHub вернул {}", response.status());
    }
    let data: serde_json::Value = response.json().await.context("некорректный ответ GitHub")?;
    let version = data
        .get("tag_name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim_start_matches('v')
        .to_string();
    if version.is_empty() {
        bail!("в релизе нет tag_name");
    }
    let wanted = asset_name()?;
    let asset = data
        .get("assets")
        .and_then(|v| v.as_array())
        .and_then(|assets| {
            assets
                .iter()
                .find(|item| item.get("name").and_then(|v| v.as_str()) == Some(wanted))
        })
        .with_context(|| format!("в релизе нет {wanted}"))?;
    let url = asset
        .get("browser_download_url")
        .and_then(|v| v.as_str())
        .context("у ассета нет browser_download_url")?
        .to_string();
    let size = asset.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
    let digest = asset
        .get("digest")
        .and_then(|v| v.as_str())
        .map(ToString::to_string);
    Ok(ReleaseInfo {
        version,
        url,
        name: wanted.to_string(),
        size,
        digest,
        published_at: data
            .get("published_at")
            .and_then(|v| v.as_str())
            .map(ToString::to_string),
        page: data
            .get("html_url")
            .and_then(|v| v.as_str())
            .map(ToString::to_string),
    })
}

fn allowed_host(host: &str) -> bool {
    matches!(
        host,
        "github.com" | "objects.githubusercontent.com" | "release-assets.githubusercontent.com"
    ) || host.ends_with(".githubusercontent.com")
}

fn valid_binary(head: &[u8]) -> bool {
    if cfg!(windows) {
        return head.starts_with(b"MZ");
    }
    if cfg!(target_os = "macos") {
        let Some(magic) = head.get(0..4) else {
            return false;
        };
        return matches!(
            magic,
            [0xcf, 0xfa, 0xed, 0xfe]
                | [0xfe, 0xed, 0xfa, 0xcf]
                | [0xca, 0xfe, 0xba, 0xbe]
                | [0xbe, 0xba, 0xfe, 0xca]
                | [0xce, 0xfa, 0xed, 0xfe]
                | [0xfe, 0xed, 0xfa, 0xce]
        );
    }
    true
}

pub async fn download(dir: &Path, info: &ReleaseInfo) -> Result<PathBuf> {
    let filename = if cfg!(windows) {
        "wireproxy-control.update.exe"
    } else {
        "wireproxy-control.update"
    };
    let target = dir.join(filename);
    let part = dir.join(format!("{filename}.part"));
    let _ = tokio::fs::remove_file(&part).await;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .redirect(reqwest::redirect::Policy::limited(10))
        .user_agent(format!(
            "wireproxy-control/{} updater",
            env!("CARGO_PKG_VERSION")
        ))
        .build()?;
    let response = client
        .get(&info.url)
        .send()
        .await
        .context("не удалось скачать обновление")?;
    let final_url = response.url().clone();
    if final_url.scheme() != "https" {
        bail!("обновление должно скачиваться по https");
    }
    if let Some(host) = final_url.host_str() {
        if !allowed_host(host) {
            bail!("неожиданный хост загрузки обновления: {host}");
        }
    } else {
        bail!("в URL обновления нет хоста");
    }
    let response = response.error_for_status().context("сервер отдал ошибку")?;
    let total = response.content_length().unwrap_or(0);
    if total > MAX_ASSET {
        bail!("обновление больше лимита");
    }
    let mut file = tokio::fs::File::create(&part).await?;
    let mut hasher = Sha256::new();
    let mut written = 0u64;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("обрыв загрузки обновления")?;
        written += chunk.len() as u64;
        if written > MAX_ASSET {
            let _ = tokio::fs::remove_file(&part).await;
            bail!("обновление больше лимита");
        }
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    drop(file);
    if info.size > 0 && written != info.size {
        let _ = tokio::fs::remove_file(&part).await;
        bail!(
            "размер обновления {written} не совпадает с релизом {}",
            info.size
        );
    }
    let actual = hex::encode(hasher.finalize());
    if let Some(digest) = &info.digest {
        let expected = digest.strip_prefix("sha256:").unwrap_or(digest);
        if !actual.eq_ignore_ascii_case(expected) {
            let _ = tokio::fs::remove_file(&part).await;
            bail!("sha256 обновления не совпадает");
        }
    }
    let mut head = [0u8; 4];
    let read = std::fs::File::open(&part)
        .and_then(|mut file| file.read(&mut head))
        .unwrap_or(0);
    if !valid_binary(&head[..read]) {
        let _ = tokio::fs::remove_file(&part).await;
        bail!("скачанный файл не похож на бинарник для этой ОС");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&part, std::fs::Permissions::from_mode(0o755));
    }
    let _ = tokio::fs::remove_file(&target).await;
    tokio::fs::rename(&part, &target).await?;
    Ok(target)
}

#[cfg(windows)]
fn shell_safe(value: &str) -> bool {
    !value.contains(['&', '|', '<', '>', '^', '%', '"', '\r', '\n'])
}

/// Запускает процесс отдельно, без shell-конкатенации пользовательских строк.
#[cfg(windows)]
fn run_detached(exe: &Path, args: &[std::ffi::OsString]) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, CREATE_NO_WINDOW, PROCESS_INFORMATION, STARTUPINFOW,
    };

    fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
        value.encode_wide().chain(std::iter::once(0)).collect()
    }
    fn quote(value: &std::ffi::OsStr) -> String {
        let text = value.to_string_lossy();
        if text.is_empty() {
            "\"\"".to_string()
        } else if text.chars().any(char::is_whitespace) {
            format!("\"{text}\"")
        } else {
            text.into_owned()
        }
    }

    let exe_text = exe.to_string_lossy();
    if !shell_safe(&exe_text) {
        bail!("путь содержит символы, недопустимые для обновления");
    }
    for arg in args {
        if !shell_safe(&arg.to_string_lossy()) {
            bail!("аргумент содержит недопустимые символы");
        }
    }
    let exe_w = wide(exe.as_os_str());
    let mut command_line = quote(exe.as_os_str());
    for arg in args {
        command_line.push(' ');
        command_line.push_str(&quote(arg));
    }
    let mut command_w: Vec<u16> = std::ffi::OsStr::new(&command_line)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
    startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    let mut process: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        CreateProcessW(
            exe_w.as_ptr(),
            command_w.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0,
            CREATE_NO_WINDOW,
            std::ptr::null(),
            std::ptr::null(),
            &startup,
            &mut process,
        )
    };
    if ok == 0 {
        bail!("CreateProcessW: {}", std::io::Error::last_os_error());
    }
    unsafe {
        CloseHandle(process.hThread);
        CloseHandle(process.hProcess);
    }
    Ok(())
}

#[cfg(not(windows))]
fn run_detached(exe: &Path, args: &[std::ffi::OsString]) -> Result<()> {
    let mut cmd = std::process::Command::new("/usr/bin/env");
    cmd.arg(exe)
        .args(args)
        .current_dir(std::env::temp_dir())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    cmd.spawn().context("не удалось запустить процесс")?;
    Ok(())
}

pub fn spawn_installer(state: &AppState, downloaded: &Path) -> Result<()> {
    if !downloaded.is_file() {
        bail!("скачанный обновлятор не найден");
    }
    let downloaded = downloaded.canonicalize()?;
    let data_dir = state
        .dir
        .canonicalize()
        .unwrap_or_else(|_| state.dir.clone());
    if downloaded.parent() != Some(data_dir.as_path()) {
        bail!("обновлятор должен лежать в каталоге данных");
    }
    let target = std::env::current_exe()?.canonicalize()?;
    let args = vec![
        std::ffi::OsString::from("--data-dir"),
        data_dir.into_os_string(),
        std::ffi::OsString::from("--panel-port"),
        std::ffi::OsString::from(state.panel_port.to_string()),
        std::ffi::OsString::from("apply-update"),
        std::ffi::OsString::from("--pid"),
        std::ffi::OsString::from(std::process::id().to_string()),
        std::ffi::OsString::from("--target"),
        target.into_os_string(),
    ];
    run_detached(&downloaded, &args)
}

#[cfg(windows)]
fn pid_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject};
    const SYNCHRONIZE: u32 = 0x0010_0000;
    if pid == 0 {
        return false;
    }
    unsafe {
        let handle = OpenProcess(SYNCHRONIZE, 0, pid);
        if handle.is_null() {
            return false;
        }
        let result = WaitForSingleObject(handle, 0);
        CloseHandle(handle);
        result == 258 // WAIT_TIMEOUT
    }
}

#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let rc = unsafe { libc::kill(pid as i32, 0) };
    if rc == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

#[cfg(not(any(windows, unix)))]
fn pid_alive(_pid: u32) -> bool {
    false
}

fn wait_for_exit(pid: u32) -> Result<()> {
    for _ in 0..600 {
        if !pid_alive(pid) {
            std::thread::sleep(Duration::from_millis(500));
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    bail!("менеджер не завершился за 2 минуты")
}

fn start_target(target: &Path, data_dir: &Path, panel_port: u16) -> Result<()> {
    let args = vec![
        std::ffi::OsString::from("--hidden"),
        std::ffi::OsString::from("--data-dir"),
        data_dir.as_os_str().to_owned(),
        std::ffi::OsString::from("--panel-port"),
        std::ffi::OsString::from(panel_port.to_string()),
    ];
    run_detached(target, &args)
}

#[cfg(target_os = "macos")]
fn resign_app(target: &Path) {
    let Some(macos) = target.parent() else { return };
    if macos.file_name().and_then(|v| v.to_str()) != Some("MacOS") {
        return;
    }
    let Some(contents) = macos.parent() else {
        return;
    };
    let Some(app) = contents.parent() else { return };
    if app.extension().and_then(|v| v.to_str()) != Some("app") {
        return;
    }
    match std::process::Command::new("codesign")
        .args(["--force", "--deep", "--sign", "-"])
        .arg(app)
        .status()
    {
        Ok(status) if status.success() => wlog!("обновление: .app переподписан"),
        Ok(status) => wlog!("обновление: codesign вернул {status}"),
        Err(e) => wlog!("обновление: codesign — {e}"),
    }
}

#[cfg(not(target_os = "macos"))]
fn resign_app(_target: &Path) {}

fn install_files(source: &Path, target: &Path) -> Result<()> {
    let file_name = target
        .file_name()
        .and_then(|v| v.to_str())
        .context("некорректный путь к бинарнику")?;
    let staged = target.with_file_name(format!(".{file_name}.new"));
    let backup = target.with_file_name(format!("{file_name}.old"));
    let _ = std::fs::remove_file(&staged);
    let _ = std::fs::remove_file(&backup);
    std::fs::copy(source, &staged).context("не удалось подготовить новый бинарник")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755));
    }
    if target.exists() {
        std::fs::rename(target, &backup).context("не удалось сохранить старый бинарник")?;
    }
    if let Err(e) = std::fs::rename(&staged, target) {
        if backup.exists() {
            let _ = std::fs::rename(&backup, target);
        }
        return Err(e).context("не удалось заменить бинарник");
    }
    resign_app(target);
    Ok(())
}

/// Вызывается новым бинарником как `apply-update`: ждёт выхода старого
/// менеджера, заменяет его файл и запускает новую версию.
pub fn apply_pending_update(
    pid: u32,
    target: &Path,
    data_dir: &Path,
    panel_port: u16,
) -> Result<()> {
    if !target.is_file() {
        bail!("целевой бинарник не найден: {}", target.display());
    }
    wait_for_exit(pid)?;
    let source = std::env::current_exe()?.canonicalize()?;
    if let Err(e) = install_files(&source, target) {
        wlog!("обновление: {e:#}; запускаю старую версию");
        let _ = start_target(target, data_dir, panel_port);
        return Err(e);
    }
    match start_target(target, data_dir, panel_port) {
        Ok(()) => {
            if let Some(name) = target.file_name().and_then(|v| v.to_str()) {
                let backup = target.with_file_name(format!("{name}.old"));
                let _ = std::fs::remove_file(backup);
            }
            wlog!("обновление: новая версия запущена");
            Ok(())
        }
        Err(e) => {
            let name = target
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("wireproxy-control");
            let backup = target.with_file_name(format!("{name}.old"));
            if backup.is_file() {
                let _ = std::fs::remove_file(target);
                let _ = std::fs::rename(&backup, target);
                let _ = start_target(target, data_dir, panel_port);
            }
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare() {
        assert!(version_tuple("0.5.4") > version_tuple("0.5.3"));
        assert!(version_tuple("v0.6.0") > version_tuple("0.5.99"));
        assert_eq!(version_tuple("1.2"), (1, 2, 0));
    }

    #[test]
    fn installs_files_and_keeps_backup_name() {
        let dir = std::env::temp_dir().join(format!("wpc-update-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("new.bin");
        let target = dir.join("app.bin");
        std::fs::write(&source, b"new-content").unwrap();
        std::fs::write(&target, b"old-content").unwrap();
        install_files(&source, &target).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new-content");
        assert_eq!(
            std::fs::read(dir.join("app.bin.old")).unwrap(),
            b"old-content"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
