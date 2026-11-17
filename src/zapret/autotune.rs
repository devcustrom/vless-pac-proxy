use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;
#[cfg(windows)]
use std::process::Command;
use std::sync::Arc;
#[cfg(windows)]
use std::time::Duration;

#[cfg(windows)]
use anyhow::Context;
use anyhow::{bail, Result};

use crate::state::AppState;
#[cfg(windows)]
use crate::wlog;

#[cfg(windows)]
use super::preset::{self, BlockcheckPaths};
#[cfg(windows)]
use super::service;
use super::AutoTuneState;

const LOG_FILE: &str = "zapret-blockcheck.log";
const DONE_FILE: &str = "zapret-blockcheck.done";
const CMD_FILE: &str = "zapret-blockcheck.cmd";

fn set_state(state: &AppState, f: impl FnOnce(&mut AutoTuneState)) {
    if let Ok(mut guard) = state.zapret_auto.lock() {
        f(&mut guard);
    }
}

fn tail(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(18)..].join("\n")
}

#[cfg(windows)]
fn cygpath(exe: &Path, path: &Path) -> Result<String> {
    let output = Command::new(exe)
        .args(["-C", "OEM", "-a", "-u"])
        .arg(path)
        .output()
        .context("не удалось запустить cygpath")?;
    if !output.status.success() {
        bail!("cygpath: {}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(windows)]
fn build_cmd(data_dir: &Path, paths: &BlockcheckPaths) -> Result<PathBuf> {
    let script = cygpath(&paths.cygpath, &paths.script)?;
    let log = cygpath(&paths.cygpath, &data_dir.join(LOG_FILE))?;
    let cygwin_bin = paths.bash.parent().context("нет каталога cygwin")?;
    let done = data_dir.join(DONE_FILE);
    let log_win = data_dir.join(LOG_FILE);
    let cmd_text = format!(
        "@echo off\r\n\
         setlocal\r\n\
         set \"PATH={};%PATH%\"\r\n\
         set \"BATCH=1\"\r\n\
         set \"CURL_CMD=1\"\r\n\
         set \"SKIP_DNSCHECK=1\"\r\n\
         set \"SCANLEVEL=standard\"\r\n\
         set \"IPV=4\"\r\n\
         set \"DOMAINS=www.youtube.com discord.com\"\r\n\
         del \"{}\" 2>nul\r\n\
         del \"{}\" 2>nul\r\n\
         \"{}\" -l -c \"'{}' 2>&1 | tee '{}'\"\r\n\
         echo %errorlevel%>\"{}\"\r\n",
        cygwin_bin.display(),
        log_win.display(),
        done.display(),
        paths.bash.display(),
        script,
        log,
        done.display(),
    );
    let path = data_dir.join(CMD_FILE);
    std::fs::write(&path, cmd_text)?;
    Ok(path)
}

fn parse_strategy(log: &str) -> Option<String> {
    let mut fallback = None;
    for line in log.lines() {
        let lower = line.to_ascii_lowercase();
        let Some(pos) = lower.find(" : winws2 ") else {
            continue;
        };
        let args = line[pos + " : winws2 ".len()..].trim();
        if args.is_empty()
            || lower.contains("not working")
            || lower.contains("working without bypass")
            || lower.contains("test aborted")
        {
            continue;
        }
        if lower.contains("https") {
            return Some(args.to_string());
        }
        fallback.get_or_insert_with(|| args.to_string());
    }
    fallback
}

pub fn start(state: &Arc<AppState>) -> Result<()> {
    #[cfg(not(windows))]
    {
        let _ = state;
        bail!("Zapret поддерживается только на Windows");
    }
    #[cfg(windows)]
    {
        if state.zapret_auto.lock().map(|v| v.running).unwrap_or(false) {
            bail!("автоподбор уже выполняется");
        }
        let paths = preset::blockcheck_paths(&state.dir)
            .context("для автоподбора нужен zapret2-бандл с blockcheck2.sh и cygwin")?;
        let cmd = build_cmd(&state.dir, &paths)?;
        let _ = std::fs::remove_file(state.dir.join(DONE_FILE));
        let _ = std::fs::remove_file(state.dir.join(LOG_FILE));
        set_state(state, |s| {
            *s = AutoTuneState {
                running: true,
                phase: "blockcheck".to_string(),
                log: String::new(),
                error: None,
                strategy: None,
                started_at: Some(chrono::Utc::now().to_rfc3339()),
                finished_at: None,
            };
        });
        if let Err(e) = crate::shell::run_elevated(&cmd) {
            set_state(state, |s| {
                s.running = false;
                s.phase = "error".to_string();
                s.error = Some(format!("{e:#}"));
            });
            return Err(e);
        }
        let st = state.clone();
        tokio::spawn(async move { monitor(st).await });
        Ok(())
    }
}

#[cfg(windows)]
async fn monitor(state: Arc<AppState>) {
    let done = state.dir.join(DONE_FILE);
    let log = state.dir.join(LOG_FILE);
    let mut finished = false;
    for step in 0..900 {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let snapshot = tail(&log);
        set_state(&state, |s| s.log = snapshot);
        if done.is_file() {
            finished = true;
            break;
        }
        if step >= 45 && !log.is_file() {
            set_state(&state, |s| {
                s.running = false;
                s.phase = "error".to_string();
                s.error = Some("blockcheck не запустился: UAC не подтверждён?".to_string());
                s.finished_at = Some(chrono::Utc::now().to_rfc3339());
            });
            return;
        }
    }
    if !finished {
        set_state(&state, |s| {
            s.running = false;
            s.phase = "error".to_string();
            s.error = Some("blockcheck не завершился за 30 минут".to_string());
            s.finished_at = Some(chrono::Utc::now().to_rfc3339());
        });
        return;
    }
    let content = std::fs::read_to_string(&log).unwrap_or_default();
    let Some(strategy) = parse_strategy(&content) else {
        set_state(&state, |s| {
            s.running = false;
            s.phase = "error".to_string();
            s.error = Some("рабочая стратегия не найдена".to_string());
            s.log = tail(&log);
            s.finished_at = Some(chrono::Utc::now().to_rfc3339());
        });
        return;
    };
    let args = preset::split_args(&strategy);
    set_state(&state, |s| {
        s.phase = "installing".to_string();
        s.strategy = Some(strategy.clone());
        s.log = tail(&log);
    });
    wlog!(
        "zapret: автоподбор нашёл стратегию ({} аргументов)",
        args.len()
    );
    if let Err(e) = service::install_args(&state.dir, args) {
        set_state(&state, |s| {
            s.running = false;
            s.phase = "error".to_string();
            s.error = Some(format!("{e:#}"));
            s.finished_at = Some(chrono::Utc::now().to_rfc3339());
        });
        return;
    }
    let mut running = false;
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        if service::query(&state.dir).running {
            running = true;
            break;
        }
    }
    set_state(&state, |s| {
        s.running = false;
        s.phase = if running { "done" } else { "error" }.to_string();
        if !running {
            s.error = Some("служба не запустилась после установки стратегии".to_string());
        }
        s.log = tail(&log);
        s.finished_at = Some(chrono::Utc::now().to_rfc3339());
    });
}

#[cfg(not(windows))]
async fn monitor(_state: Arc<AppState>) {}

#[cfg(test)]
mod tests {
    use super::parse_strategy;

    #[test]
    fn picks_working_https_strategy() {
        let log = r#"
- http ipv4 www.youtube.com : winws2 --wf-tcp-out=80 --lua-desync=fake
- https ipv4 www.youtube.com : winws2 --wf-tcp-out=443 --lua-desync=fake --new
- https ipv4 discord.com : winws2 not working
"#;
        let strategy = parse_strategy(log).unwrap();
        assert!(strategy.contains("--wf-tcp-out=443"));
        assert!(!strategy.contains("not working"));
    }

    #[test]
    fn no_strategy_returns_none() {
        let log = "- https ipv4 www.youtube.com : winws2 not working\n";
        assert!(parse_strategy(log).is_none());
    }
}
