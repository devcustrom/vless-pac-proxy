use std::path::Path;
#[cfg(windows)]
use std::process::Command;

#[cfg(windows)]
use anyhow::Context;
use anyhow::{bail, Result};
use serde::Serialize;

#[cfg(windows)]
use base64::Engine;

pub const SERVICE_NAME: &str = "wireproxy-zapret";
const LOG_FILE: &str = "zapret-service.log";

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    pub installed: bool,
    pub running: bool,
    pub state: String,
    pub error: Option<String>,
    pub log: String,
}

fn log_path(data_dir: &Path) -> std::path::PathBuf {
    data_dir.join(LOG_FILE)
}

fn tail_log(data_dir: &Path) -> String {
    let text = std::fs::read_to_string(log_path(data_dir)).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(40)..].join("\n")
}

#[cfg(windows)]
fn sc(args: &[&str]) -> Result<String> {
    use std::os::windows::process::CommandExt;
    let output = Command::new("sc.exe")
        .args(args)
        .creation_flags(0x08000000)
        .output()
        .context("не удалось запустить sc.exe")?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        bail!("{}", text.trim());
    }
    Ok(text)
}

#[cfg(windows)]
pub fn query(data_dir: &Path) -> ServiceStatus {
    use std::os::windows::process::CommandExt;
    let output = Command::new("sc.exe")
        .args(["query", SERVICE_NAME])
        .creation_flags(0x08000000)
        .output();
    let mut status = ServiceStatus {
        log: tail_log(data_dir),
        ..Default::default()
    };
    let Ok(output) = output else {
        status.error = Some("не удалось выполнить sc.exe query".to_string());
        return status;
    };
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        status.state = "not installed".to_string();
        return status;
    }
    // sc.exe локализован, но коды состояний (RUNNING/STOPPED/…) остаются
    // английскими. Не привязываемся к слову SERVICE_NAME/STATE.
    status.installed = true;
    let (state, running) = parse_state(&text);
    status.state = state;
    status.running = running;
    status
}

#[cfg(windows)]
fn parse_state(text: &str) -> (String, bool) {
    const STATES: [&str; 7] = [
        "STOP_PENDING",
        "START_PENDING",
        "PAUSE_PENDING",
        "CONTINUE_PENDING",
        "RUNNING",
        "STOPPED",
        "PAUSED",
    ];
    for line in text.lines() {
        let upper = line.to_ascii_uppercase();
        if let Some(state) = STATES.iter().find(|state| upper.contains(**state)) {
            return ((*state).to_string(), *state == "RUNNING");
        }
    }
    ("unknown".to_string(), false)
}

#[cfg(not(windows))]
pub fn query(data_dir: &Path) -> ServiceStatus {
    ServiceStatus {
        error: Some("Zapret поддерживается только на Windows".to_string()),
        log: tail_log(data_dir),
        ..Default::default()
    }
}

#[cfg(windows)]
pub fn start() -> Result<String> {
    let text = sc(&["start", SERVICE_NAME])?;
    Ok(text.trim().to_string())
}

#[cfg(windows)]
pub fn stop() -> Result<String> {
    let text = sc(&["stop", SERVICE_NAME])?;
    Ok(text.trim().to_string())
}

#[cfg(not(windows))]
pub fn start() -> Result<String> {
    bail!("Zapret поддерживается только на Windows")
}

#[cfg(not(windows))]
pub fn stop() -> Result<String> {
    bail!("Zapret поддерживается только на Windows")
}

#[cfg(windows)]
const INSTALL_PS1: &str = r#"$ErrorActionPreference = 'Continue'
$log = Join-Path $PSScriptRoot '__LOG__'
function Write-Log([string]$text) {
    $line = (Get-Date -Format 'yyyy-MM-dd HH:mm:ss') + '  ' + $text
    Add-Content -LiteralPath $log -Value $line -Encoding UTF8
}
$payload = [Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('__PAYLOAD__')) | ConvertFrom-Json
$svc = 'wireproxy-zapret'
$sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$argLine = ($payload.args | ForEach-Object {
    if ($_ -match '\s') { '"' + $_ + '"' } else { $_ }
}) -join ' '
$binPath = '"' + $payload.bin + '" ' + $argLine
Write-Log "Installing service $svc"
Write-Log "binPath=$binPath"
& netsh interface tcp set global timestamps=enabled 2>&1 | Out-Null
& sc.exe stop $svc 2>&1 | Out-Null
Start-Sleep -Milliseconds 800
& sc.exe delete $svc 2>&1 | Out-Null
Start-Sleep -Milliseconds 700
$out = & sc.exe create $svc binPath= $binPath DisplayName= 'wireproxy-control Zapret' start= demand 2>&1
$out | ForEach-Object { Write-Log $_ }
$sddl = "D:(A;;CCLCSWRPWPDTLOCRRC;;;SY)(A;;CCDCLCSWRPWPDTLOCRSDRCWDWO;;;BA)(A;;CCLCSWLORPWP;;;IU)(A;;CCLCSWRPWPDTLOCRRC;;;$sid)"
$out = & sc.exe sdset $svc $sddl 2>&1
$out | ForEach-Object { Write-Log $_ }
Start-Sleep -Milliseconds 300
$out = & sc.exe start $svc 2>&1
$out | ForEach-Object { Write-Log $_ }
Write-Log "Done"
"#;

#[cfg(windows)]
const REMOVE_PS1: &str = r#"$ErrorActionPreference = 'Continue'
$log = Join-Path $PSScriptRoot '__LOG__'
function Write-Log([string]$text) {
    $line = (Get-Date -Format 'yyyy-MM-dd HH:mm:ss') + '  ' + $text
    Add-Content -LiteralPath $log -Value $line -Encoding UTF8
}
$svc = 'wireproxy-zapret'
Write-Log "Removing service $svc"
& sc.exe stop $svc 2>&1 | ForEach-Object { Write-Log $_ }
Start-Sleep -Milliseconds 800
& sc.exe delete $svc 2>&1 | ForEach-Object { Write-Log $_ }
Write-Log "Removed"
"#;

#[cfg(windows)]
const CMD_WRAPPER: &str =
    "@echo off\r\npowershell -NoProfile -ExecutionPolicy Bypass -File \"%~dp0__PS1__\"\r\n";

#[cfg(windows)]
fn write_script(data_dir: &Path, template: &str, ps1: &str) -> Result<std::path::PathBuf> {
    let ps1_path = data_dir.join(ps1);
    let cmd_path = data_dir.join(format!("{}.cmd", ps1.trim_end_matches(".ps1")));
    std::fs::write(&ps1_path, template)
        .with_context(|| format!("не удалось записать {}", ps1_path.display()))?;
    let wrapper = CMD_WRAPPER.replace("__PS1__", ps1);
    std::fs::write(&cmd_path, wrapper)
        .with_context(|| format!("не удалось записать {}", cmd_path.display()))?;
    Ok(cmd_path)
}

#[cfg(windows)]
pub fn install(data_dir: &Path, preset_name: &str) -> Result<()> {
    if !super::preset::is_bundle_installed(data_dir) {
        bail!("бандл Zapret не найден: распакуйте Flowseal/zapret2 в папку zapret");
    }
    super::preset::ensure_user_lists(data_dir)?;
    let args = super::preset::parse_preset(data_dir, preset_name)?;
    install_args(data_dir, args)
}

#[cfg(windows)]
pub fn install_args(data_dir: &Path, args: Vec<String>) -> Result<()> {
    let bin = super::preset::engine_path(data_dir)?;
    install_args_for(data_dir, &bin, args)
}

/// Установка аргументов с явно выбранным движком: каталог Zaprett использует
/// `winws.exe` (v1) даже в бандле zapret2, где рядом лежит `winws2.exe`.
#[cfg(windows)]
pub fn install_args_for(data_dir: &Path, bin: &Path, args: Vec<String>) -> Result<()> {
    if !super::preset::is_bundle_installed(data_dir) {
        bail!("бандл Zapret не найден: распакуйте Flowseal/zapret2 в папку zapret");
    }
    if args.is_empty() {
        bail!("пустая стратегия Zapret");
    }
    if !bin.is_file() {
        bail!("движок Zapret не найден: {}", bin.display());
    }
    let payload = serde_json::json!({
        "bin": bin.display().to_string(),
        "args": args,
    });
    let encoded = base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&payload)?);
    let script = INSTALL_PS1
        .replace("__LOG__", LOG_FILE)
        .replace("__PAYLOAD__", &encoded);
    let cmd = write_script(data_dir, &script, "zapret-service.ps1")?;
    crate::shell::run_elevated(&cmd)?;
    Ok(())
}

#[cfg(windows)]
pub fn remove(data_dir: &Path) -> Result<()> {
    let script = REMOVE_PS1.replace("__LOG__", LOG_FILE);
    let cmd = write_script(data_dir, &script, "zapret-service-remove.ps1")?;
    crate::shell::run_elevated(&cmd)?;
    Ok(())
}

#[cfg(not(windows))]
pub fn install(_data_dir: &Path, _preset_name: &str) -> Result<()> {
    bail!("Zapret поддерживается только на Windows")
}

#[cfg(not(windows))]
pub fn install_args(_data_dir: &Path, _args: Vec<String>) -> Result<()> {
    bail!("Zapret поддерживается только на Windows")
}

#[cfg(not(windows))]
pub fn install_args_for(_data_dir: &Path, _bin: &Path, _args: Vec<String>) -> Result<()> {
    bail!("Zapret поддерживается только на Windows")
}

#[cfg(not(windows))]
pub fn remove(_data_dir: &Path) -> Result<()> {
    bail!("Zapret поддерживается только на Windows")
}

#[cfg(all(test, windows))]
mod tests {
    use super::parse_state;

    #[test]
    fn parses_localized_sc_state() {
        let text = "Имя_службы: winmgmt\n        Тип                : 30  WIN32\n        Состояние          : 4  RUNNING\n";
        assert_eq!(parse_state(text), ("RUNNING".to_string(), true));
        let stopped = "SERVICE_NAME: wireproxy-zapret\n        STATE              : 1  STOPPED\n";
        assert_eq!(parse_state(stopped), ("STOPPED".to_string(), false));
        assert_eq!(
            parse_state("SERVICE_NAME: x\n"),
            ("unknown".to_string(), false)
        );
    }
}
