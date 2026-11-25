use std::collections::HashSet;
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(windows)]
use std::time::Duration;

use anyhow::{bail, Context, Result};

use crate::state::AppState;
#[cfg(windows)]
use crate::wlog;

use super::preset;
#[cfg(windows)]
use super::preset::BlockcheckPaths;
#[cfg(windows)]
use super::service;
use super::{AutoTuneState, StrategyCandidate};

const LOG_FILE: &str = "zapret-blockcheck.log";
const DONE_FILE: &str = "zapret-blockcheck.done";
const CMD_FILE: &str = "zapret-blockcheck.cmd";
const DOMAINS_FILE: &str = "zapret-blockcheck.domains";
const SCAN_FILE: &str = "zapret-blockcheck.scan";
const DEFAULT_DOMAINS: &str = "www.youtube.com discord.com";
const MAX_CANDIDATES: usize = 40;

fn set_state(state: &AppState, f: impl FnOnce(&mut AutoTuneState)) {
    if let Ok(mut guard) = state.zapret_auto.lock() {
        f(&mut guard);
    }
}

fn fail(state: &AppState, message: impl Into<String>) {
    let message = message.into();
    set_state(state, |s| {
        s.running = false;
        s.phase = "error".to_string();
        s.error = Some(message);
        s.finished_at = Some(chrono::Utc::now().to_rfc3339());
    });
    #[cfg(windows)]
    restore_service(state);
}

#[cfg(windows)]
fn restore_service(state: &AppState) {
    let paused = state
        .zapret_auto
        .lock()
        .map(|s| s.service_paused)
        .unwrap_or(false);
    if !paused {
        return;
    }
    if service::query(&state.dir).running {
        return;
    }
    if let Err(e) = service::start() {
        wlog!("zapret: не удалось вернуть службу после blockcheck — {e:#}");
    }
}

fn tail(path: &Path) -> String {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(18)..].join("\n")
}

fn normalize_scan(value: &str) -> &'static str {
    match value {
        "veryquick" => "veryquick",
        "quick" => "quick",
        "force" => "force",
        _ => "standard",
    }
}

/// Убирает Windows verbatim-префикс `\\?\`: cygwin и cmd не понимают его
/// в путях и в PATH.
fn plain_path(path: &Path) -> String {
    let text = path.to_string_lossy();
    text.strip_prefix("\\\\?\\").unwrap_or(&text).to_string()
}

/// Cygwin понимает пути Windows с прямыми слэшами; отдельный cygpath.exe
/// не нужен, поэтому внешняя команда с путём из данных не запускается.
fn posix_path(path: &Path) -> String {
    plain_path(path).replace('\\', "/")
}

/// Корень бандла по пути к cygwin/bash: `<root>\cygwin\bin\bash.exe`.
#[cfg(windows)]
fn zapret_root(paths: &BlockcheckPaths) -> Option<&Path> {
    paths.bash.parent()?.parent()?.parent()
}

/// Убивает только тестовые процессы из каталога Zapret. Широкий
/// `taskkill /IM winws2.exe` задевал бы пользовательскую службу, а одного
/// `bash.exe` мало: blockcheck2 оставляет `sh`, `tee`, `curl` и `winws2`,
/// которые продолжают перезапускать друг друга и ломают новые соединения.
#[cfg(windows)]
fn cleanup_script(root: &Path) -> String {
    let root = plain_path(root).replace('\'', "''");
    format!(
        "$ErrorActionPreference='SilentlyContinue'\n\
         $root='{root}'\n\
         $roots=@((Join-Path $root 'cygwin'),(Join-Path $root 'blockcheck'),(Join-Path $root 'zapret-winws'))\n\
         function Test-WpcUnderRoot([string]$path) {{\n\
         \x20 foreach ($item in $roots) {{ if ($path.StartsWith($item, [StringComparison]::OrdinalIgnoreCase)) {{ return $true }} }}\n\
         \x20 return $false\n\
         }}\n\
         for ($pass=0; $pass -lt 6; $pass++) {{\n\
         \x20 $targets=Get-CimInstance Win32_Process | Where-Object {{\n\
         \x20   $path=$_.ExecutablePath\n\
         \x20   $cmd=$_.CommandLine\n\
         \x20   ($path -and (Test-WpcUnderRoot $path)) -or\n\
         \x20   ($_.Name -eq 'bash.exe' -and $cmd -like '*blockcheck2.sh*') -or\n\
         \x20   ($_.Name -eq 'wireproxy-control.exe' -and $cmd -like '*CatalogTest*')\n\
         \x20 }}\n\
         \x20 if (-not $targets) {{ break }}\n\
         \x20 $targets | ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force }}\n\
         \x20 Start-Sleep -Milliseconds 500\n\
         }}\n"
    )
}

#[cfg(windows)]
fn write_cleanup_script(data_dir: &Path, root: &Path) -> Result<PathBuf> {
    let path = data_dir.join("zapret-blockcheck-cleanup.ps1");
    std::fs::write(&path, cleanup_script(root))?;
    Ok(path)
}

/// Очистка из уже Elevated-процесса (helper проверки каталога).
#[cfg(windows)]
pub(crate) fn cleanup_test_processes(data_dir: &Path) -> Result<()> {
    let root = preset::bundle_dir(data_dir);
    let path = data_dir.join("zapret-test-cleanup.ps1");
    std::fs::write(&path, cleanup_script(&root))?;
    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(&path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("не удалось запустить очистку тестовых процессов Zapret")?;
    if !status.success() {
        bail!("очистка тестовых процессов Zapret завершилась с ошибкой");
    }
    Ok(())
}

#[cfg(windows)]
fn build_cmd(
    data_dir: &Path,
    paths: &BlockcheckPaths,
    domains: &str,
    scan: &str,
    restart_service: bool,
) -> Result<PathBuf> {
    if domains.is_empty()
        || domains
            .chars()
            .any(|c| !(c.is_ascii_alphanumeric() || c == ' ' || c == '.' || c == '-'))
    {
        bail!("некорректный список доменов для blockcheck");
    }
    // Домен и уровень сканирования передаём файлами: в текст .cmd они не
    // попадают, поэтому даже специальные символы не станут командой.
    let domains_path = data_dir.join(DOMAINS_FILE);
    let scan_path = data_dir.join(SCAN_FILE);
    std::fs::write(&domains_path, format!("{domains}\n"))?;
    std::fs::write(&scan_path, format!("{}\n", normalize_scan(scan)))?;
    let script = posix_path(&paths.script);
    let log = posix_path(&data_dir.join(LOG_FILE));
    let cygwin_bin = paths.bash.parent().context("нет каталога cygwin")?;
    let done = data_dir.join(DONE_FILE);
    let log_win = data_dir.join(LOG_FILE);
    let root = zapret_root(paths)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| preset::bundle_dir(data_dir));
    let cleanup_path = write_cleanup_script(data_dir, &root)?;
    let cleanup = plain_path(&cleanup_path);
    let cmd_text = format!(
        "@echo off\r\n\
         setlocal\r\n\
         set \"PATH={};%PATH%\"\r\n\
         set \"BATCH=1\"\r\n\
         set \"CURL_CMD=1\"\r\n\
         set \"SKIP_DNSCHECK=1\"\r\n\
         set \"ENABLE_HTTP=0\"\r\n\
         set \"ENABLE_HTTPS_TLS12=1\"\r\n\
         set \"ENABLE_HTTPS_TLS13=0\"\r\n\
         set \"ENABLE_HTTP3=0\"\r\n\
         set \"CURL_MAX_TIME=1\"\r\n\
         set \"CURL_MAX_TIME_DOH=1\"\r\n\
         set /p SCANLEVEL=<\"{}\"\r\n\
         set \"IPV=4\"\r\n\
         set /p DOMAINS=<\"{}\"\r\n\
         set \"WPC_RESTART={}\"\r\n\
         sc stop wireproxy-zapret >nul 2>&1\r\n\
         ping 127.0.0.1 -n 3 >nul\r\n\
         powershell -NoProfile -ExecutionPolicy Bypass -File \"{}\" >nul 2>&1\r\n\
         del \"{}\" 2>nul\r\n\
         del \"{}\" 2>nul\r\n\
         \"{}\" -l -c \"'{}' 2>&1 | tee '{}'\"\r\n\
         powershell -NoProfile -ExecutionPolicy Bypass -File \"{}\" >nul 2>&1\r\n\
         if \"%WPC_RESTART%\"==\"1\" sc start wireproxy-zapret >nul 2>&1\r\n\
         echo %errorlevel%>\"{}\"\r\n",
        plain_path(cygwin_bin),
        plain_path(&scan_path),
        plain_path(&domains_path),
        if restart_service { 1 } else { 0 },
        cleanup,
        plain_path(&log_win),
        plain_path(&done),
        plain_path(&paths.bash),
        script,
        log,
        cleanup,
        plain_path(&done),
    );
    let path = data_dir.join(CMD_FILE);
    std::fs::write(&path, cmd_text)?;
    Ok(path)
}

fn valid_candidate_args(args: &[String]) -> bool {
    !args.is_empty()
        && args.iter().all(|arg| arg.starts_with("--"))
        && args
            .iter()
            .any(|arg| arg.starts_with("--filter-") || arg.starts_with("--wf-"))
        && !args
            .iter()
            .any(|arg| arg.contains("exit_guard") || arg.contains("timer_set"))
}

fn candidate_from_line(line: &str) -> Option<(String, String)> {
    let lower = line.to_ascii_lowercase();
    let pos = lower.find(" : winws2 ")?;
    let prefix = line[..pos]
        .trim()
        .trim_matches(|c: char| c == '!' || c == '-' || c.is_whitespace());
    let args = line[pos + " : winws2 ".len()..]
        .trim()
        .trim_end_matches(|c: char| c == '!' || c.is_whitespace());
    if args.is_empty() {
        return None;
    }
    let tokens = preset::split_args(args);
    if !valid_candidate_args(&tokens) {
        return None;
    }
    Some((prefix.to_string(), args.to_string()))
}

fn parse_candidates(log: &str) -> Vec<StrategyCandidate> {
    // Основной источник — SUMMARY. Если его нет (лог обрезан/blockcheck
    // завершился нестандартно), берём стратегии, помеченные AVAILABLE.
    let section = log
        .split_once("* SUMMARY")
        .map(|(_, rest)| rest)
        .unwrap_or("");
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for line in section.lines() {
        let Some((prefix, args)) = candidate_from_line(line) else {
            continue;
        };
        if !seen.insert(args.clone()) {
            continue;
        }
        let label = if prefix.is_empty() {
            format!("Стратегия {}", out.len() + 1)
        } else {
            prefix
        };
        out.push(StrategyCandidate {
            index: out.len(),
            label,
            strategy: args,
        });
        if out.len() >= MAX_CANDIDATES {
            break;
        }
    }
    if out.is_empty() {
        out = parse_available(log);
    }
    out
}

/// Разбирает пары «- curl_test ... : winws2 ...» + «!!!!! AVAILABLE !!!!!».
fn parse_available(log: &str) -> Vec<StrategyCandidate> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut pending: Option<(String, String)> = None;
    for line in log.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("- ") {
            pending = candidate_from_line(rest.trim());
            continue;
        }
        if trimmed.contains("!!!!! AVAILABLE !!!!!") {
            let Some((prefix, args)) = pending.take() else {
                continue;
            };
            if !seen.insert(args.clone()) {
                continue;
            }
            let label = if prefix.is_empty() {
                format!("Стратегия {}", out.len() + 1)
            } else {
                prefix
            };
            out.push(StrategyCandidate {
                index: out.len(),
                label,
                strategy: args,
            });
            if out.len() >= MAX_CANDIDATES {
                break;
            }
        }
    }
    out
}

fn select_best(candidates: &[StrategyCandidate]) -> Option<&StrategyCandidate> {
    candidates
        .iter()
        .find(|item| item.label.contains("https_tls13"))
        .or_else(|| candidates.iter().find(|item| item.label.contains("https")))
        .or_else(|| candidates.first())
}

fn parse_strategy(log: &str) -> Option<String> {
    select_best(&parse_candidates(log)).map(|item| item.strategy.clone())
}

fn strategy_args(raw: &str) -> Result<Vec<String>> {
    let args = preset::split_args(raw);
    if !valid_candidate_args(&args) {
        bail!("стратегия содержит недопустимые аргументы");
    }
    Ok(args)
}

pub fn start(state: &Arc<AppState>) -> Result<()> {
    // veryquick останавливает перебор вариантов семейства после первого
    // успеха. Этого достаточно для одной кнопки и не растягивает поиск
    // на десятки минут.
    start_inner(state, None, "veryquick")
}

/// Генерирует стратегии blockcheck2 для конкретного домена. В отличие от
/// общего автоподбора, ничего не устанавливает: кандидаты возвращаются в
/// состоянии и применяются из UI.
pub fn start_for_domain(state: &Arc<AppState>, domain: &str, scan: &str) -> Result<()> {
    let Some(domain) = crate::lists::validate_hostname(domain) else {
        bail!("некорректный домен для генерации стратегий");
    };
    if domain == "localhost" || domain.ends_with(".localhost") || domain.ends_with(".local") {
        bail!("для локального домена стратегия не нужна");
    }
    start_inner(state, Some(domain), scan)
}

fn start_inner(state: &Arc<AppState>, domain: Option<String>, scan: &str) -> Result<()> {
    #[cfg(not(windows))]
    {
        let _ = (state, domain, scan);
        bail!("Zapret поддерживается только на Windows");
    }
    #[cfg(windows)]
    {
        {
            let guard = state
                .zapret_auto
                .lock()
                .map_err(|_| anyhow::anyhow!("состояние автоподбора повреждено"))?;
            if guard.running {
                bail!("автоподбор уже выполняется");
            }
        }
        let paths = match preset::blockcheck_paths(&state.dir)
            .context("для генерации нужен zapret2-бандл с blockcheck2.sh и cygwin")
        {
            Ok(paths) => paths,
            Err(e) => {
                fail(state, format!("{e:#}"));
                return Err(e);
            }
        };
        let was_running = service::query(&state.dir).running;
        let domains = domain.clone().unwrap_or_else(|| {
            let mut configured = state.settings_snapshot().zapret_domains;
            // blockcheck проверяет каждую стратегию по каждому домену.
            // Три цели — разумный компромисс; остальные проверяются
            // точечно через диалог стратегий.
            configured.truncate(3);
            if configured.is_empty() {
                DEFAULT_DOMAINS.to_string()
            } else {
                configured.join(" ")
            }
        });
        let cmd = match build_cmd(
            &state.dir,
            &paths,
            &domains,
            normalize_scan(scan),
            was_running,
        ) {
            Ok(cmd) => cmd,
            Err(e) => {
                fail(state, format!("{e:#}"));
                return Err(e);
            }
        };
        set_state(state, |s| {
            *s = AutoTuneState {
                running: true,
                phase: "starting".to_string(),
                log: String::new(),
                error: None,
                strategy: None,
                domain: domain.clone(),
                candidates: Vec::new(),
                preset: None,
                service_paused: was_running,
                started_at: Some(chrono::Utc::now().to_rfc3339()),
                finished_at: None,
            };
        });
        let _ = std::fs::remove_file(state.dir.join(DONE_FILE));
        let _ = std::fs::remove_file(state.dir.join(LOG_FILE));
        set_state(state, |s| {
            s.phase = "blockcheck".to_string();
        });
        if let Err(e) = crate::shell::run_elevated(&cmd) {
            fail(state, format!("{e:#}"));
            return Err(e);
        }
        let st = state.clone();
        tokio::spawn(async move { monitor(st).await });
        Ok(())
    }
}

#[cfg(windows)]
fn strategy_globals(state: &AppState, args: &[String]) -> Vec<String> {
    let mut globals = Vec::new();
    if !args.iter().any(|arg| arg == "--wf-tcp-empty=1") {
        globals.push("--wf-tcp-empty=1".to_string());
    }
    if let Ok(dir) = preset::engine_dir(&state.dir) {
        for lua in ["zapret-lib.lua", "zapret-antidpi.lua"] {
            if args.iter().any(|arg| arg.contains(lua)) {
                continue;
            }
            let path = dir.join("lua").join(lua);
            if path.is_file() {
                globals.push(format!("--lua-init=@{}", path.display()));
            }
        }
    }
    globals
}

#[cfg(windows)]
fn install_strategy(state: &AppState, args: &[String]) -> Result<()> {
    let mut full = strategy_globals(state, args);
    full.extend(args.iter().cloned());
    service::install_args(&state.dir, full)
}

#[cfg(windows)]
async fn wait_started(state: Arc<AppState>) {
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
        s.finished_at = Some(chrono::Utc::now().to_rfc3339());
    });
    // Стратегия, подобранная для одного домена, не покрывает весь список.
    let single_domain = state
        .zapret_auto
        .lock()
        .map(|s| s.domain.is_some())
        .unwrap_or(false);
    if running && !single_domain {
        let stamp = crate::zapret::domains_stamp(&state.settings_snapshot().zapret_domains);
        let mut settings = state.settings.lock().unwrap();
        settings.zapret_strategy_stamp = stamp;
        if let Err(e) = settings.save(&state.dir) {
            wlog!("zapret: не удалось сохранить отпечаток стратегии — {e:#}");
        }
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
            fail(&state, "blockcheck не запустился: UAC не подтверждён?");
            return;
        }
    }
    if !finished {
        fail(&state, "blockcheck не завершился за 30 минут");
        return;
    }
    let content = std::fs::read_to_string(&log).unwrap_or_default();
    if content.trim().is_empty() {
        fail(
            &state,
            "blockcheck не создал лог: проверьте UAC, cygwin и путь к бандлу",
        );
        return;
    }
    let candidates = parse_candidates(&content);
    if candidates.is_empty() {
        set_state(&state, |s| {
            s.running = false;
            s.phase = "error".to_string();
            s.error = Some("рабочие стратегии не найдены".to_string());
            s.log = tail(&log);
            s.candidates = Vec::new();
            s.finished_at = Some(chrono::Utc::now().to_rfc3339());
        });
        restore_service(&state);
        return;
    }
    let domain_mode = state
        .zapret_auto
        .lock()
        .map(|s| s.domain.is_some())
        .unwrap_or(false);
    set_state(&state, |s| {
        s.candidates = candidates.clone();
        s.log = tail(&log);
    });
    if domain_mode {
        set_state(&state, |s| {
            s.running = false;
            s.phase = "done".to_string();
            s.error = None;
            s.strategy = None;
            s.finished_at = Some(chrono::Utc::now().to_rfc3339());
        });
        wlog!(
            "zapret: сгенерировано {} стратегий для домена",
            candidates.len()
        );
        restore_service(&state);
        return;
    }
    let Some(best) = select_best(&candidates) else {
        fail(&state, "рабочая стратегия не найдена");
        return;
    };
    let strategy = best.strategy.clone();
    let args = match strategy_args(&strategy) {
        Ok(args) => args,
        Err(e) => {
            fail(&state, format!("{e:#}"));
            return;
        }
    };
    set_state(&state, |s| {
        s.phase = "installing".to_string();
        s.strategy = Some(strategy.clone());
    });
    wlog!(
        "zapret: автоподбор нашёл стратегию ({} аргументов)",
        args.len()
    );
    if let Err(e) = install_strategy(&state, &args) {
        fail(&state, format!("{e:#}"));
        return;
    }
    wait_started(state).await;
}

/// Применяет найденный кандидат к службе. Аргументы берутся из состояния
/// (а не из запроса), поэтому подменить их через API нельзя.
pub fn apply_candidate(state: &Arc<AppState>, index: usize) -> Result<()> {
    #[cfg(not(windows))]
    {
        let _ = (state, index);
        bail!("Zapret поддерживается только на Windows");
    }
    #[cfg(windows)]
    {
        let (candidate, running) = {
            let guard = state
                .zapret_auto
                .lock()
                .map_err(|_| anyhow::anyhow!("состояние автоподбора повреждено"))?;
            let candidate = guard
                .candidates
                .get(index)
                .cloned()
                .context("стратегия не найдена")?;
            (candidate, guard.running)
        };
        if running {
            bail!("дождитесь завершения генерации");
        }
        let args = strategy_args(&candidate.strategy)?;
        set_state(state, |s| {
            s.running = true;
            s.phase = "installing".to_string();
            s.error = None;
            s.strategy = Some(candidate.strategy.clone());
            s.preset = None;
            s.started_at = Some(chrono::Utc::now().to_rfc3339());
            s.finished_at = None;
        });
        if let Err(e) = install_strategy(state, &args) {
            fail(state, format!("{e:#}"));
            return Err(e);
        }
        let st = state.clone();
        tokio::spawn(async move { wait_started(st).await });
        Ok(())
    }
}

/// Сохраняет найденный кандидат как пресет zapret2, чтобы его можно было
/// выбрать вручную или объединить с другими стратегиями.
pub fn save_candidate(state: &Arc<AppState>, index: usize) -> Result<String> {
    let (candidate, domain, running) = {
        let guard = state
            .zapret_auto
            .lock()
            .map_err(|_| anyhow::anyhow!("состояние автоподбора повреждено"))?;
        let candidate = guard
            .candidates
            .get(index)
            .cloned()
            .context("стратегия не найдена")?;
        (candidate, guard.domain.clone(), guard.running)
    };
    if running {
        bail!("дождитесь завершения генерации");
    }
    let domain = domain.context("стратегия не привязана к домену")?;
    let args = strategy_args(&candidate.strategy)?;
    let name = preset::write_generated_preset(&state.dir, &domain, &args)?;
    set_state(state, |s| s.preset = Some(name.clone()));
    Ok(name)
}

#[cfg(not(windows))]
async fn monitor(_state: Arc<AppState>) {}

#[cfg(test)]
mod tests {
    use super::{parse_candidates, parse_strategy, plain_path, posix_path};
    use std::path::Path;

    #[test]
    fn strips_windows_verbatim_prefix() {
        let path = Path::new(r"\\?\C:\Users\dev\zapret\cygwin\bin\bash.exe");
        assert_eq!(plain_path(path), r"C:\Users\dev\zapret\cygwin\bin\bash.exe");
        assert_eq!(posix_path(path), "C:/Users/dev/zapret/cygwin/bin/bash.exe");
    }

    #[test]
    fn picks_working_https_strategy() {
        let log = r#"
!!!!! curl_test_https_tls13: working strategy found for ipv4 www.youtube.com : winws2 --wf-tcp-out=443 --filter-tcp=443 --lua-desync=fake !!!!!
* SUMMARY
curl_test_http ipv4 www.youtube.com : winws2 --wf-tcp-out=80 --filter-tcp=80 --lua-desync=fake
curl_test_https_tls13 ipv4 www.youtube.com : winws2 --wf-tcp-out=443 --filter-tcp=443 --lua-desync=fake
curl_test_https_tls12 ipv4 discord.com : winws2 --wf-tcp-out=443 --filter-tcp=443 --lua-desync=fake
"#;
        let candidates = parse_candidates(log);
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].label, "curl_test_http ipv4 www.youtube.com");
        let strategy = parse_strategy(log).unwrap();
        assert!(strategy.contains("--wf-tcp-out=443"));
        assert!(strategy.contains("--filter-tcp=443"));
        assert!(!strategy.contains('!'));
    }

    #[test]
    fn ignores_broken_and_unsafe_summary_lines() {
        let log = r#"
* SUMMARY
curl_test_https_tls13 ipv4 example.com : winws2 not working
curl_test_https_tls13 ipv4 example.com : winws2 calc.exe --filter-tcp=443
curl_test_https_tls13 ipv4 example.com : winws2 --lua-init=timer_set('x') --filter-tcp=443
"#;
        assert!(parse_candidates(log).is_empty());
        assert!(parse_strategy(log).is_none());
    }

    #[test]
    fn no_summary_returns_none() {
        let log = "- https ipv4 www.youtube.com : winws2 --filter-tcp=443\n";
        assert!(parse_candidates(log).is_empty());
        assert!(parse_strategy(log).is_none());
    }

    #[test]
    fn falls_back_to_available_lines() {
        let log = r#"
- curl_test_https_tls12 ipv4 www.youtube.com : winws2 --wf-l3=ipv4 --wf-tcp-out=443 --payload=tls_client_hello --lua-desync=fake
curl: (28) Connection timed out after 2007 milliseconds
UNAVAILABLE code=28
- curl_test_https_tls12 ipv4 www.youtube.com : winws2 --wf-l3=ipv4 --wf-tcp-out=443 --payload=tls_client_hello --lua-desync=multisplit
curl -w %{time_total}
!!!!! AVAILABLE !!!!!
"#;
        let candidates = parse_candidates(log);
        assert_eq!(candidates.len(), 1);
        assert_eq!(
            candidates[0].label,
            "curl_test_https_tls12 ipv4 www.youtube.com"
        );
        assert!(candidates[0].strategy.contains("--lua-desync=multisplit"));
    }

    #[cfg(windows)]
    #[test]
    fn blockcheck_cmd_uses_plain_paths() {
        use super::{build_cmd, BlockcheckPaths};
        use std::path::PathBuf;

        let base = std::env::temp_dir().join(format!("wpc-blockcheck-cmd-{}", std::process::id()));
        std::fs::create_dir_all(base.join("cygwin").join("bin")).unwrap();
        let verbatim = PathBuf::from(format!("\\\\?\\{}", base.display()));
        let paths = BlockcheckPaths {
            bash: base.join("cygwin").join("bin").join("bash.exe"),
            cygpath: base.join("cygwin").join("bin").join("cygpath.exe"),
            script: base.join("blockcheck2.sh"),
            winws2: base.join("winws2.exe"),
        };
        let cmd = build_cmd(&verbatim, &paths, "example.com", "quick", false).unwrap();
        let text = std::fs::read_to_string(cmd).unwrap();
        assert!(!text.contains("\\\\?\\"));
        assert!(!text.contains("//?/"));
        assert!(text.contains("set /p DOMAINS"));
        assert!(text.contains(
            &base
                .join("blockcheck2.sh")
                .to_string_lossy()
                .replace('\\', "/")
        ));
        // Широкий taskkill убивал бы пользовательскую службу Zapret.
        assert!(!text.contains("taskkill /F /IM winws2.exe"));
        assert!(!text.contains("taskkill /F /IM winws.exe"));
        // Очистка запускается до и после blockcheck.
        assert_eq!(text.matches("zapret-blockcheck-cleanup.ps1").count(), 2);
        let cleanup = std::fs::read_to_string(base.join("zapret-blockcheck-cleanup.ps1")).unwrap();
        assert!(cleanup.contains(&base.to_string_lossy().to_string()));
        for dir in ["cygwin", "blockcheck", "zapret-winws"] {
            assert!(cleanup.contains(&format!("'{}'", dir)));
        }
        assert!(cleanup.contains("blockcheck2.sh"));
        assert!(cleanup.contains("Test-WpcUnderRoot"));
        let _ = std::fs::remove_dir_all(base);
    }

    #[cfg(windows)]
    #[test]
    fn cleanup_script_escapes_quotes_and_targets_test_dirs() {
        use super::cleanup_script;
        use std::path::Path;

        let script = cleanup_script(Path::new(r"C:\Users\O'Brien\zapret"));
        assert!(script.contains(r"C:\Users\O''Brien\zapret"));
        for dir in ["cygwin", "blockcheck", "zapret-winws"] {
            assert!(script.contains(&format!("'{}'", dir)));
        }
        assert!(script.contains("Stop-Process -Id"));
    }
}
