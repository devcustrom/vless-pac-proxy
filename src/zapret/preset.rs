use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleKind {
    /// Flowseal general*.bat + bin/winws.exe.
    V1,
    /// zapret2: zapret-winws/winws2.exe + lua + preset2*.cmd (+ blockcheck2).
    V2,
}

#[derive(Debug, Clone, Serialize)]
pub struct PresetInfo {
    pub name: String,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct BlockcheckPaths {
    pub bash: PathBuf,
    pub cygpath: PathBuf,
    pub script: PathBuf,
    pub winws2: PathBuf,
}

pub fn bundle_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("zapret")
}

pub fn bundle_kind(data_dir: &Path) -> Option<BundleKind> {
    let root = bundle_dir(data_dir);
    if root.join("zapret-winws").join("winws2.exe").is_file() || root.join("winws2.exe").is_file() {
        Some(BundleKind::V2)
    } else if root.join("bin").join("winws.exe").is_file() {
        Some(BundleKind::V1)
    } else {
        None
    }
}

pub fn engine_path(data_dir: &Path) -> Result<PathBuf> {
    let root = bundle_dir(data_dir);
    match bundle_kind(data_dir) {
        Some(BundleKind::V2) if root.join("zapret-winws").join("winws2.exe").is_file() => {
            Ok(root.join("zapret-winws").join("winws2.exe"))
        }
        Some(BundleKind::V2) => Ok(root.join("winws2.exe")),
        Some(BundleKind::V1) => Ok(root.join("bin").join("winws.exe")),
        None => bail!("не найден winws.exe или winws2.exe в папке zapret"),
    }
}

pub fn engine_label(data_dir: &Path) -> &'static str {
    match bundle_kind(data_dir) {
        Some(BundleKind::V2) => "winws2",
        Some(BundleKind::V1) => "winws",
        None => "none",
    }
}

pub fn engine_dir(data_dir: &Path) -> Result<PathBuf> {
    engine_path(data_dir)?
        .parent()
        .map(Path::to_path_buf)
        .context("нет каталога движка Zapret")
}

fn preset_dir(data_dir: &Path) -> Result<PathBuf> {
    match bundle_kind(data_dir) {
        Some(BundleKind::V2) => engine_dir(data_dir),
        Some(BundleKind::V1) => Ok(bundle_dir(data_dir)),
        None => bail!("бандл Zapret не найден"),
    }
}

pub fn is_bundle_installed(data_dir: &Path) -> bool {
    let Ok(engine) = engine_path(data_dir) else {
        return false;
    };
    let Some(dir) = engine.parent() else {
        return false;
    };
    engine.is_file() && dir.join("WinDivert.dll").is_file() && dir.join("WinDivert64.sys").is_file()
}

/// Создаёт пользовательские списки Flowseal v1, которые general*.bat ожидает увидеть.
pub fn ensure_user_lists(data_dir: &Path) -> Result<()> {
    if bundle_kind(data_dir) != Some(BundleKind::V1) {
        return Ok(());
    }
    let lists = bundle_dir(data_dir).join("lists");
    std::fs::create_dir_all(&lists)?;
    let files = [
        ("ipset-exclude-user.txt", "203.0.113.113/32\n"),
        (
            "list-general-user.txt",
            "# Never leave this file empty\ndomain.example.abc\n",
        ),
        ("list-exclude-user.txt", "domain.example.abc\n"),
    ];
    for (name, content) in files {
        let path = lists.join(name);
        if !path.exists() {
            std::fs::write(&path, content)?;
        }
    }
    Ok(())
}

pub fn list_presets(data_dir: &Path) -> Vec<PresetInfo> {
    let Ok(dir) = preset_dir(data_dir) else {
        return Vec::new();
    };
    let kind = bundle_kind(data_dir);
    let mut items = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return items;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let lower = name.to_ascii_lowercase();
        let include = match kind {
            Some(BundleKind::V2) => {
                lower.starts_with("preset2") && (lower.ends_with(".cmd") || lower.ends_with(".bat"))
            }
            Some(BundleKind::V1) => lower.starts_with("general") && lower.ends_with(".bat"),
            None => false,
        };
        if !include {
            continue;
        }
        items.push(PresetInfo {
            label: name
                .trim_end_matches(".bat")
                .trim_end_matches(".cmd")
                .to_string(),
            name,
        });
    }
    items.sort_by(|a, b| {
        let a_primary = a.name.eq_ignore_ascii_case("general.bat")
            || a.name.eq_ignore_ascii_case("preset2_example.cmd");
        let b_primary = b.name.eq_ignore_ascii_case("general.bat")
            || b.name.eq_ignore_ascii_case("preset2_example.cmd");
        b_primary.cmp(&a_primary).then_with(|| {
            a.name
                .to_ascii_lowercase()
                .cmp(&b.name.to_ascii_lowercase())
        })
    });
    items
}

pub fn valid_preset_name(data_dir: &Path, name: &str) -> bool {
    let path = Path::new(name);
    if path.file_name().and_then(|v| v.to_str()) != Some(name) {
        return false;
    }
    let lower = name.to_ascii_lowercase();
    match bundle_kind(data_dir) {
        Some(BundleKind::V2) => {
            lower.starts_with("preset2") && (lower.ends_with(".cmd") || lower.ends_with(".bat"))
        }
        Some(BundleKind::V1) => lower.starts_with("general") && lower.ends_with(".bat"),
        None => false,
    }
}

fn tokenize(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for ch in line.chars() {
        match ch {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out.retain(|arg| !arg.is_empty());
    for arg in &mut out {
        if arg.ends_with(',') {
            arg.pop();
        }
    }
    out
}

pub fn split_args(line: &str) -> Vec<String> {
    tokenize(line)
}

fn extract_args(text: &str, base_dir: &Path, marker: &str) -> Option<Vec<String>> {
    let base = format!("{}\\", base_dir.display());
    let bin = format!("{}\\", base_dir.join("bin").display());
    let lists = format!("{}\\", base_dir.join("lists").display());
    let mut joined = String::new();
    for raw in text.lines() {
        let line = raw.trim_end();
        if joined.is_empty() {
            joined.push_str(line);
        } else {
            joined.push(' ');
            joined.push_str(line.trim_start());
        }
        if joined.trim_end().ends_with('^') {
            let trimmed = joined.trim_end();
            joined = trimmed[..trimmed.len() - 1].trim_end().to_string();
            continue;
        }
        let lower = joined.to_ascii_lowercase();
        if let Some(pos) = lower.find(marker) {
            let rest = joined[pos + marker.len()..]
                .trim_start_matches('"')
                .trim_start();
            let expanded = rest
                .replace("%~dp0", &base)
                .replace("%BIN%", &bin)
                .replace("%LISTS%", &lists)
                .replace("%GameFilterTCP%", "")
                .replace("%GameFilterUDP%", "");
            let args = tokenize(&expanded);
            if !args.is_empty() {
                return Some(args);
            }
        }
        joined.clear();
    }
    None
}

pub fn parse_preset(data_dir: &Path, name: &str) -> Result<Vec<String>> {
    if !valid_preset_name(data_dir, name) {
        bail!("некорректное имя пресета");
    }
    let kind = bundle_kind(data_dir).context("бандл Zapret не найден")?;
    let dir = preset_dir(data_dir)?;
    let path = dir.join(name);
    let text = std::fs::read_to_string(&path)
        .with_context(|| format!("не удалось прочитать {}", path.display()))?;
    let marker = match kind {
        BundleKind::V2 => "winws2.exe",
        BundleKind::V1 => "winws.exe",
    };
    extract_args(&text, &dir, marker).context("в пресете не найдена команда winws.exe/winws2.exe")
}

pub fn blockcheck_paths(data_dir: &Path) -> Option<BlockcheckPaths> {
    if bundle_kind(data_dir) != Some(BundleKind::V2) {
        return None;
    }
    let root = bundle_dir(data_dir);
    let bash = root.join("cygwin").join("bin").join("bash.exe");
    let cygpath = root.join("cygwin").join("bin").join("cygpath.exe");
    let script = root
        .join("blockcheck")
        .join("zapret2")
        .join("blockcheck2.sh");
    let winws2 = engine_path(data_dir).ok()?;
    if bash.is_file() && cygpath.is_file() && script.is_file() {
        Some(BlockcheckPaths {
            bash,
            cygpath,
            script,
            winws2,
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "wpc-zapret-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parses_general_bat_command() {
        let dir = temp();
        let text = r#"
@echo off
set "BIN=%~dp0bin\"
set "LISTS=%~dp0lists\"
start "zapret: test" /min "%BIN%winws.exe" --wf-tcp=80,443,%GameFilterTCP% ^
--filter-tcp=443 --hostlist="%LISTS%list-general.txt" --new
"#;
        let args = extract_args(text, &dir, "winws.exe").unwrap();
        assert_eq!(args[0], "--wf-tcp=80,443");
        assert!(args.iter().any(|a| a == "--filter-tcp=443"));
        assert!(args
            .iter()
            .any(|a| a.ends_with("list-general.txt") && a.starts_with("--hostlist=")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn parses_preset2_command() {
        let dir = temp();
        let text = r#"start "zapret2" /min "%~dp0winws2.exe" ^
--wf-tcp-out=80,443 ^
--lua-init=@"%~dp0lua\zapret-lib.lua" --lua-init=@"%~dp0lua\zapret-antidpi.lua" ^
--filter-tcp=443 --filter-l7=tls --lua-desync=fake --new
"#;
        let args = extract_args(text, &dir, "winws2.exe").unwrap();
        assert_eq!(args[0], "--wf-tcp-out=80,443");
        assert!(args.iter().any(|a| a.contains("zapret-lib.lua")));
        assert!(args.iter().any(|a| a == "--new"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_bad_preset_names() {
        let dir = temp();
        assert!(!valid_preset_name(&dir, "../general.bat"));
        assert!(!valid_preset_name(&dir, "service.bat"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
