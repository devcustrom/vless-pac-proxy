//! Синхронизация hosts для локальных доменов: `*.localhost` браузеры
//! резолвят сами, остальным именам нужна строка `127.0.0.1 domain` в hosts.
//!
//! Бинарник не повышает права: он пишет данные (`hosts-entries.txt`) и готовый
//! скрипт (`local-domains-hosts.bat` / `.command`), который пользователь
//! запускает сам (скрипт запросит UAC/пароль администратора).
use std::path::{Path, PathBuf};

use anyhow::Result;

/// Строки для hosts: только «настоящие» имена (не `*.localhost`).
pub fn entries(domains: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for domain in domains {
        let d = domain.trim().to_ascii_lowercase();
        if d.is_empty() || d == "localhost" || d.ends_with(".localhost") {
            continue;
        }
        if !d
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        {
            continue;
        }
        if !out.iter().any(|x| x.ends_with(&d)) {
            out.push(format!("127.0.0.1 {d}"));
        }
    }
    out
}

const BLOCK_BEGIN: &str = "# wireproxy-control local domains begin";
const BLOCK_END: &str = "# wireproxy-control local domains end";

#[cfg(windows)]
const PS1: &str = r#"# Adds local domains from hosts-entries.txt to the Windows hosts file.
# Run local-domains-hosts.bat as Administrator (this script needs admin rights).
$ErrorActionPreference = 'Stop'
$dir = Split-Path -Parent $MyInvocation.MyCommand.Path
$hostsPath = Join-Path $env:SystemRoot 'System32\drivers\etc\hosts'
$entriesPath = Join-Path $dir 'hosts-entries.txt'

$entries = New-Object System.Collections.Generic.List[string]
if (Test-Path -LiteralPath $entriesPath) {
    foreach ($line in Get-Content -LiteralPath $entriesPath) {
        $t = $line.Trim()
        if ($t) { $entries.Add($t) }
    }
}

$lines = New-Object System.Collections.Generic.List[string]
if (Test-Path -LiteralPath $hostsPath) {
    foreach ($l in Get-Content -LiteralPath $hostsPath) { $lines.Add($l) }
}

$out = New-Object System.Collections.Generic.List[string]
$skip = $false
foreach ($l in $lines) {
    if ($l.Trim() -eq '# wireproxy-control local domains begin') { $skip = $true; continue }
    if ($l.Trim() -eq '# wireproxy-control local domains end') { $skip = $false; continue }
    if (-not $skip) { $out.Add($l) }
}

if ($entries.Count -gt 0) {
    $out.Add('# wireproxy-control local domains begin')
    foreach ($e in $entries) { $out.Add($e) }
    $out.Add('# wireproxy-control local domains end')
}

Set-Content -LiteralPath $hostsPath -Value $out -Encoding ASCII
Write-Host "hosts updated: $($entries.Count) entries"
"#;

#[cfg(windows)]
const BAT: &str = "@echo off\r\n\
net session >nul 2>&1\r\n\
if errorlevel 1 (\r\n\
  echo Requesting administrator rights...\r\n\
  powershell -NoProfile -Command \"Start-Process -FilePath '%~f0' -Verb RunAs\"\r\n\
  exit /b\r\n\
)\r\n\
powershell -NoProfile -ExecutionPolicy Bypass -File \"%~dp0local-domains-hosts.ps1\"\r\n\
pause\r\n";

#[cfg(not(windows))]
const SH: &str = "#!/bin/sh\n\
set -e\n\
DIR=\"$(cd \"$(dirname \"$0\")\" && pwd)\"\n\
TMP=\"$(mktemp)\"\n\
trap 'rm -f \"$TMP\"' EXIT\n\
awk -v b='# wireproxy-control local domains begin' -v e='# wireproxy-control local domains end' \\\n\
  '$0 == b { skip = 1; next } $0 == e { skip = 0; next } !skip { print }' \\\n\
  /etc/hosts > \"$TMP\"\n\
if [ -s \"$DIR/hosts-entries.txt\" ]; then\n\
  printf '%s\\n' '# wireproxy-control local domains begin' >> \"$TMP\"\n\
  cat \"$DIR/hosts-entries.txt\" >> \"$TMP\"\n\
  printf '%s\\n' '# wireproxy-control local domains end' >> \"$TMP\"\n\
fi\n\
sudo cp \"$TMP\" /etc/hosts\n\
echo \"hosts updated\"\n";

/// Имя готового скрипта синхронизации hosts (для панели/трея).
pub fn hosts_script_name() -> &'static str {
    if cfg!(windows) {
        "local-domains-hosts.bat"
    } else {
        "local-domains-hosts.command"
    }
}

/// Пишет `hosts-entries.txt` и скрипт синхронизации; возвращает путь к скрипту.
pub fn write_script(dir: &Path, domains: &[String]) -> Result<PathBuf> {
    let entries = entries(domains);
    let mut data = String::new();
    for line in &entries {
        data.push_str(line);
        data.push('\n');
    }
    std::fs::write(dir.join("hosts-entries.txt"), data)?;

    #[cfg(windows)]
    {
        let ps1 = dir.join("local-domains-hosts.ps1");
        let bat = dir.join(hosts_script_name());
        std::fs::write(&ps1, PS1)?;
        std::fs::write(&bat, BAT)?;
        Ok(bat)
    }
    #[cfg(not(windows))]
    {
        let script = dir.join(hosts_script_name());
        std::fs::write(&script, SH)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755));
        }
        Ok(script)
    }
}

/// Готовит скрипт по текущему `local-domains.txt` и печатает инструкцию.
pub fn sync_hosts(dir: &Path) -> Result<()> {
    let names: Vec<String> = crate::paths::in_dir(dir, "local-domains.txt")
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|text| {
            crate::net::local_proxy::parse_config(&text)
                .into_iter()
                .map(|d| d.domain)
                .collect()
        })
        .unwrap_or_default();
    let script = write_script(dir, &names)?;
    println!(
        "Скрипт синхронизации hosts: {}\nЗапустите его (запросит UAC/пароль), чтобы имена из local-domains.txt резолвились в 127.0.0.1.",
        script.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_localhost_and_dupes() {
        let list = vec![
            "panel.localhost".to_string(),
            "local.stage.stich.su".to_string(),
            "LOCAL.stage.stich.su".to_string(),
            "bad domain".to_string(),
        ];
        assert_eq!(entries(&list), vec!["127.0.0.1 local.stage.stich.su"]);
    }

    #[test]
    fn writes_entries_and_script() {
        let dir = std::env::temp_dir().join(format!("wpc-hosts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = write_script(&dir, &["panel.localhost".into(), "api.test".into()]).unwrap();
        assert!(script.is_file());
        let data = std::fs::read_to_string(dir.join("hosts-entries.txt")).unwrap();
        assert_eq!(data, "127.0.0.1 api.test\n");
        assert!(!data.contains("panel.localhost"));
        let _ = BLOCK_BEGIN;
        let _ = BLOCK_END;
    }
}
