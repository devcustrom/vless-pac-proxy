//! Супервизор внешних движков: spawn, рестарт с задержкой, остановка,
//! стриминг лога в manager.log.
//!
//! Безопасность: имена программ и все аргументы — литералы, выбранные по
//! enum EngineKind (никакого пользовательского ввода в команду). Рабочий
//! каталог — каталог, где найден движок рядом с приложением.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;

use crate::wlog;

pub mod downloads;
pub mod wireproxy;
pub mod xray;

/// Каталог данных, где лежат скачанные движки (заполняется при старте).
static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

pub fn set_data_dir(dir: &Path) {
    let _ = DATA_DIR.set(dir.to_path_buf());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineKind {
    Wireproxy,
    Xray,
}

impl EngineKind {
    pub fn exe_name(self) -> &'static str {
        match self {
            EngineKind::Wireproxy => {
                if cfg!(windows) {
                    "wireproxy.exe"
                } else {
                    "wireproxy"
                }
            }
            EngineKind::Xray => {
                if cfg!(windows) {
                    "xray.exe"
                } else {
                    "xray"
                }
            }
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            EngineKind::Wireproxy => "wireproxy",
            EngineKind::Xray => "xray",
        }
    }
}

/// Каталоги для поиска движков: папка exe и до двух родительских уровней
/// (dev-сборка: target/release -> ... -> корень репозитория).
fn app_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        let mut cur = exe.parent().map(|p| p.to_path_buf());
        for _ in 0..3 {
            match cur {
                Some(d) => {
                    dirs.push(d.clone());
                    cur = d.parent().map(|p| p.to_path_buf());
                }
                None => break,
            }
        }
    }
    dirs
}

/// Каталог данных приоритетнее папки exe: рядом с exe могут лежать старые
/// бинарники, а конфиги (wireproxy.conf/xray-config.json) живут в данных.
pub fn find_engine(kind: EngineKind) -> Option<PathBuf> {
    if let Some(dir) = DATA_DIR.get() {
        let candidate = dir.join(kind.exe_name());
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    for dir in app_dirs() {
        let candidate = dir.join(kind.exe_name());
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn engine_dir(kind: EngineKind) -> Option<PathBuf> {
    find_engine(kind).and_then(|p| p.parent().map(|d| d.to_path_buf()))
}

enum Cmd {
    Start,
    Stop,
    Restart,
    Shutdown,
}

pub struct Engine {
    tx: mpsc::Sender<Cmd>,
    running: Arc<AtomicBool>,
}

impl Engine {
    pub fn new<F>(kind: EngineKind, configured: F) -> Self
    where
        F: Fn() -> bool + Send + 'static,
    {
        let (tx, mut rx) = mpsc::channel::<Cmd>(8);
        let running = Arc::new(AtomicBool::new(false));
        let running_task = running.clone();
        tokio::spawn(async move {
            let mut desired = false; // «процесс должен работать»
            let mut stopping = false;
            loop {
                if desired && !running_task.load(Ordering::SeqCst) && configured() {
                    match spawn_engine(kind) {
                        Ok(mut child) => {
                            running_task.store(true, Ordering::SeqCst);
                            tokio::select! {
                                status = child.wait() => {
                                    wlog!("{}: процесс завершился ({status:?})", kind.label());
                                }
                                cmd = rx.recv() => {
                                    match cmd {
                                        Some(Cmd::Start) => {}
                                        Some(Cmd::Stop) => { desired = false; let _ = child.kill().await; }
                                        Some(Cmd::Restart) => { desired = true; let _ = child.kill().await; }
                                        Some(Cmd::Shutdown) | None => {
                                            stopping = true;
                                            desired = false;
                                            let _ = child.kill().await;
                                        }
                                    }
                                    let _ = child.wait().await;
                                }
                            }
                            running_task.store(false, Ordering::SeqCst);
                            if stopping {
                                break;
                            }
                            if desired {
                                tokio::time::sleep(Duration::from_secs(3)).await;
                            }
                            continue;
                        }
                        Err(e) => {
                            // движок отсутствует/не запустился: не спамим, ждём команду
                            wlog!("{}: не удалось запустить — {e:#}", kind.label());
                            desired = false;
                            continue;
                        }
                    }
                }
                match rx.recv().await {
                    Some(Cmd::Start) | Some(Cmd::Restart) => desired = true,
                    Some(Cmd::Stop) => desired = false,
                    Some(Cmd::Shutdown) | None => break,
                }
            }
            running_task.store(false, Ordering::SeqCst);
        });
        Self { tx, running }
    }

    pub fn start(&self) {
        let _ = self.tx.try_send(Cmd::Start);
    }

    pub fn stop(&self) {
        let _ = self.tx.try_send(Cmd::Stop);
    }

    pub fn restart(&self) {
        let _ = self.tx.try_send(Cmd::Restart);
    }

    pub fn shutdown(&self) {
        let _ = self.tx.try_send(Cmd::Shutdown);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}

#[cfg(windows)]
fn build_command(kind: EngineKind) -> Command {
    match kind {
        EngineKind::Wireproxy => {
            let mut c = Command::new(".\\wireproxy.exe");
            c.args(["-c", "wireproxy.conf"]);
            c
        }
        EngineKind::Xray => {
            let mut c = Command::new(".\\xray.exe");
            c.args(["-c", "xray-config.json"]);
            c
        }
    }
}

#[cfg(not(windows))]
fn build_command(kind: EngineKind) -> Command {
    match kind {
        EngineKind::Wireproxy => {
            let mut c = Command::new("./wireproxy");
            c.args(["-c", "wireproxy.conf"]);
            c
        }
        EngineKind::Xray => {
            let mut c = Command::new("./xray");
            c.args(["-c", "xray-config.json"]);
            c
        }
    }
}

/// Рабочий каталог движка: каталог данных (там конфиги). Если бинарник лежит
/// только рядом с exe — копируем его в данные, чтобы `-c wireproxy.conf`
/// и `-c xray-config.json` находились в текущей папке.
fn work_dir(kind: EngineKind) -> Result<PathBuf> {
    if let Some(data) = DATA_DIR.get() {
        let local = data.join(kind.exe_name());
        if !local.is_file() {
            if let Some(found) = find_engine(kind) {
                std::fs::copy(&found, &local).with_context(|| {
                    format!(
                        "не удалось скопировать {} в каталог данных",
                        kind.exe_name()
                    )
                })?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ =
                        std::fs::set_permissions(&local, std::fs::Permissions::from_mode(0o755));
                }
                wlog!("{}: скопирован в каталог данных", kind.exe_name());
            }
        }
        if local.is_file() {
            return Ok(data.to_path_buf());
        }
    }
    engine_dir(kind).with_context(|| {
        format!(
            "{} не найден рядом с приложением (скачайте движки)",
            kind.exe_name()
        )
    })
}

fn spawn_engine(kind: EngineKind) -> Result<tokio::process::Child> {
    let dir = work_dir(kind)?;
    if !dir.is_dir() {
        bail!("каталог движка не найден");
    }
    let mut cmd = build_command(kind);
    cmd.current_dir(dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().context("не удалось запустить движок")?;
    wlog!("{}: запущен, pid {}", kind.label(), child.id().unwrap_or(0));
    if let Some(out) = child.stdout.take() {
        relay(kind.label().to_string(), out);
    }
    if let Some(err) = child.stderr.take() {
        relay(kind.label().to_string(), err);
    }
    Ok(child)
}

fn relay<R>(name: String, stream: R)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut lines = BufReader::new(stream).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                let short: String = trimmed.chars().take(240).collect();
                wlog!("{name}: {short}");
            }
        }
    });
}
