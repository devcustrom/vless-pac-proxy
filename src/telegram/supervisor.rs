use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use crate::state::AppState;
use crate::wlog;

use super::config::TelegramConfig;
use super::ProbeResult;

#[cfg(windows)]
mod job {
    use std::ffi::c_void;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
    };

    /// Job-объект с KILL_ON_JOB_CLOSE: PyInstaller onefile запускает
    /// bootloader + дочерний процесс, и обычный kill() оставляет child.
    pub struct Job(isize);

    impl Job {
        pub fn create() -> Option<Self> {
            let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
            if handle.is_null() {
                return None;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = unsafe {
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                )
            };
            if ok == 0 {
                unsafe { CloseHandle(handle) };
                return None;
            }
            Some(Self(handle as isize))
        }

        fn handle(&self) -> HANDLE {
            self.0 as HANDLE
        }

        pub fn assign(&self, pid: u32) -> bool {
            let process = unsafe {
                OpenProcess(
                    PROCESS_SET_QUOTA | PROCESS_TERMINATE | PROCESS_QUERY_INFORMATION,
                    0,
                    pid,
                )
            };
            if process.is_null() {
                return false;
            }
            let ok = unsafe { AssignProcessToJobObject(self.handle(), process) };
            unsafe { CloseHandle(process) };
            ok != 0
        }

        pub fn terminate(&self) {
            unsafe { TerminateJobObject(self.handle(), 1) };
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            unsafe { CloseHandle(self.handle()) };
        }
    }
}

#[cfg(not(windows))]
mod job {
    pub struct Job;

    impl Job {
        pub fn create() -> Option<Self> {
            None
        }
        pub fn assign(&self, _pid: u32) -> bool {
            false
        }
        pub fn terminate(&self) {}
    }
}

#[cfg(windows)]
pub fn sidecar_name() -> &'static str {
    "tgws-proxy.exe"
}

#[cfg(not(windows))]
pub fn sidecar_name() -> &'static str {
    "tgws-proxy"
}

pub fn sidecar_path(dir: &Path) -> PathBuf {
    dir.join(sidecar_name())
}

fn command() -> Command {
    #[cfg(windows)]
    {
        Command::new(".\\tgws-proxy.exe")
    }
    #[cfg(not(windows))]
    {
        Command::new("./tgws-proxy")
    }
}

fn spawn_child(st: &AppState, cfg: &TelegramConfig) -> Result<(Child, Option<job::Job>)> {
    if !sidecar_path(&st.dir).is_file() {
        bail!(
            "sidecar не найден: положите {} в каталог данных",
            sidecar_name()
        );
    }
    let mut cmd = command();
    cmd.args(["--config", "tgws-config.json"])
        .current_dir(&st.dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    {
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().context("не удалось запустить tgws-proxy")?;
    let pid = child.id();
    let job = job::Job::create();
    if let (Some(job), Some(pid)) = (job.as_ref(), pid) {
        if !job.assign(pid) {
            wlog!("telegram: не удалось назначить sidecar в job-объект");
        }
    }
    if let Some(out) = child.stdout.take() {
        relay("stdout", out, cfg.secret.clone());
    }
    if let Some(err) = child.stderr.take() {
        relay("stderr", err, cfg.secret.clone());
    }
    wlog!("telegram: sidecar запущен, pid {}", pid.unwrap_or(0));
    Ok((child, job))
}

fn relay<R>(name: &'static str, stream: R, secret: String)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let hex32 = regex::Regex::new(r"[0-9a-fA-F]{32}").ok();
        let mut lines = BufReader::new(stream).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let mut text = line.trim().to_string();
            if !secret.is_empty() {
                text = text.replace(&secret, "***");
            }
            if let Some(re) = &hex32 {
                text = re.replace_all(&text, "***").to_string();
            }
            if let Some(idx) = text.find("tg://proxy") {
                text.truncate(idx);
                text.push_str("tg://proxy?...");
            }
            wlog!("telegram/{name}: {text}");
        }
    });
}

pub fn start(state: &Arc<AppState>) -> Result<()> {
    let settings = state.settings_snapshot();
    if !settings.telegram_enabled {
        bail!("Telegram-прокси выключен");
    }
    let cfg = TelegramConfig::from_settings(&settings, state.panel_port)?;
    cfg.write(&state.dir)?;
    if !sidecar_path(&state.dir).is_file() {
        let msg = format!(
            "sidecar не найден: положите {} в {}",
            sidecar_name(),
            state.dir.display()
        );
        *state
            .telegram
            .error
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(msg.clone());
        bail!(msg);
    }

    let generation = state.telegram.generation.fetch_add(1, Ordering::SeqCst) + 1;
    let (tx, rx) = tokio::sync::watch::channel(false);
    {
        let mut guard = state
            .telegram
            .stop
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if guard.is_some() {
            return Ok(());
        }
        *guard = Some((generation, tx));
    }
    *state
        .telegram
        .error
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = None;
    let st = state.clone();
    tokio::spawn(async move {
        supervise(st.clone(), cfg, generation, rx).await;
    });
    Ok(())
}

async fn supervise(
    state: Arc<AppState>,
    cfg: TelegramConfig,
    generation: u64,
    mut rx: tokio::sync::watch::Receiver<bool>,
) {
    let mut backoff = 1u64;
    loop {
        if *rx.borrow() {
            break;
        }
        match spawn_child(&state, &cfg) {
            Ok((mut child, job)) => {
                let pid = child.id();
                *state.telegram.pid.lock().unwrap_or_else(|e| e.into_inner()) = pid;
                *state
                    .telegram
                    .last_started
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
                let exited = tokio::select! {
                    status = child.wait() => Some(status),
                    changed = rx.changed() => {
                        let _ = changed;
                        if let Some(job) = &job {
                            job.terminate();
                        }
                        let _ = child.kill().await;
                        let _ = child.wait().await;
                        None
                    }
                };
                *state.telegram.pid.lock().unwrap_or_else(|e| e.into_inner()) = None;
                drop(job);
                if *rx.borrow() || exited.is_none() {
                    break;
                }
                if let Some(Ok(status)) = exited {
                    wlog!("telegram: sidecar завершился ({status})");
                    *state
                        .telegram
                        .error
                        .lock()
                        .unwrap_or_else(|e| e.into_inner()) =
                        Some(format!("sidecar завершился: {status}"));
                }
                state.telegram.restarts.fetch_add(1, Ordering::Relaxed);
                tokio::time::sleep(Duration::from_secs(backoff)).await;
                backoff = (backoff * 2).min(60);
            }
            Err(e) => {
                *state
                    .telegram
                    .error
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = Some(format!("{e:#}"));
                wlog!("telegram: {e:#}");
                break;
            }
        }
    }
    let mut guard = state
        .telegram
        .stop
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if matches!(&*guard, Some((gen, _)) if *gen == generation) {
        *guard = None;
    }
}

pub fn stop(state: &AppState) {
    if let Ok(mut guard) = state.telegram.stop.lock() {
        if let Some((_, tx)) = guard.take() {
            let _ = tx.send(true);
        }
    }
    *state.telegram.pid.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

pub async fn restart(state: &Arc<AppState>) -> Result<()> {
    stop(state);
    tokio::time::sleep(Duration::from_millis(700)).await;
    start(state)?;
    tokio::time::sleep(Duration::from_millis(400)).await;
    Ok(())
}

pub async fn probe(state: &Arc<AppState>) -> ProbeResult {
    let settings = state.settings_snapshot();
    let at = chrono::Utc::now().to_rfc3339();
    let direct = state
        .telegram
        .probe
        .lock()
        .ok()
        .and_then(|p| p.clone())
        .map(|p| p.direct)
        .unwrap_or_default();
    let started = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        tokio::net::TcpStream::connect((settings.telegram_host.as_str(), settings.telegram_port)),
    )
    .await;
    let probe = match result {
        Ok(Ok(_)) => ProbeResult {
            at: Some(at),
            running: true,
            latency_ms: Some(started.elapsed().as_millis() as u64),
            error: None,
            direct,
        },
        Ok(Err(e)) => ProbeResult {
            at: Some(at),
            running: false,
            latency_ms: None,
            error: Some(e.to_string()),
            direct,
        },
        Err(_) => ProbeResult {
            at: Some(at),
            running: false,
            latency_ms: None,
            error: Some("таймаут".to_string()),
            direct,
        },
    };
    *state
        .telegram
        .probe
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(probe.clone());
    probe
}

/// Проверяет TCP-доступность дата-центров Telegram напрямую (без TLS/WS).
/// Полная WS-проверка выполняется самим sidecar при первом подключении.
pub async fn probe_direct(state: &Arc<AppState>) -> ProbeResult {
    let targets = [
        ("1", "149.154.175.50"),
        ("2", "149.154.167.220"),
        ("3", "149.154.175.100"),
        ("4", "149.154.167.220"),
        ("5", "149.154.171.5"),
        ("203", "91.105.192.100"),
    ];
    let mut direct = std::collections::BTreeMap::new();
    for (dc, ip) in targets {
        let ok = tokio::time::timeout(
            Duration::from_secs(4),
            tokio::net::TcpStream::connect((ip, 443)),
        )
        .await
        .map(|r| r.is_ok())
        .unwrap_or(false);
        direct.insert(dc.to_string(), ok);
    }
    let probe = ProbeResult {
        at: Some(chrono::Utc::now().to_rfc3339()),
        running: state
            .telegram
            .pid
            .lock()
            .map(|pid| pid.is_some())
            .unwrap_or(false),
        latency_ms: None,
        error: None,
        direct,
    };
    *state
        .telegram
        .probe
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(probe.clone());
    probe
}
