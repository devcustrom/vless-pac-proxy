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

/// Движок zapret v1 из любого бандла. В zapret2 `winws.exe` лежит рядом с
/// `winws2.exe`; он нужен для стратегий каталога Zaprett (nfqws).
pub fn engine_path_v1(data_dir: &Path) -> Result<PathBuf> {
    let root = bundle_dir(data_dir);
    let candidates = [
        root.join("bin").join("winws.exe"),
        root.join("zapret-winws").join("winws.exe"),
        root.join("winws.exe"),
    ];
    for path in candidates {
        if path.is_file() {
            return Ok(path);
        }
    }
    bail!("не найден winws.exe (zapret v1) в папке zapret")
}

/// Движок zapret2 (winws2) строго из V2-бандла.
pub fn engine_path_v2(data_dir: &Path) -> Result<PathBuf> {
    if bundle_kind(data_dir) != Some(BundleKind::V2) {
        bail!("не найден winws2.exe (zapret2) в папке zapret");
    }
    engine_path(data_dir)
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

fn add_global(globals: &mut Vec<String>, arg: String) {
    if let Some((name, value)) = arg.split_once('=') {
        if matches!(
            name,
            "--wf-tcp" | "--wf-udp" | "--wf-tcp-out" | "--wf-udp-out"
        ) {
            let prefix = format!("{name}=");
            if let Some(existing) = globals.iter_mut().find(|item| item.starts_with(&prefix)) {
                let mut values: Vec<String> = existing[prefix.len()..]
                    .split(',')
                    .filter(|item| !item.is_empty())
                    .map(ToString::to_string)
                    .collect();
                for item in value.split(',').filter(|item| !item.is_empty()) {
                    if !values.iter().any(|have| have == item) {
                        values.push(item.to_string());
                    }
                }
                *existing = format!("{prefix}{}", values.join(","));
                return;
            }
        }
    }
    if !globals.iter().any(|item| item == &arg) {
        globals.push(arg);
    }
}

/// Объединяет несколько пресетов в одну команду winws: общие аргументы
/// (--wf-*, --lua-init, --blob и т.п.) дублируются один раз, наборы
/// профилей соединяются через --new.
pub fn merge_args(groups: Vec<Vec<String>>) -> Result<Vec<String>> {
    let mut globals = Vec::new();
    let mut profiles = Vec::new();
    for args in groups {
        let split = args
            .iter()
            .position(|arg| arg.starts_with("--filter-"))
            .context("в пресете нет --filter-* стратегий")?;
        let (head, mut tail) = args.split_at(split);
        for arg in head {
            add_global(&mut globals, arg.clone());
        }
        while tail.last().map(|arg| arg == "--new").unwrap_or(false) {
            tail = &tail[..tail.len() - 1];
        }
        if tail.is_empty() {
            bail!("в пресете нет стратегий");
        }
        if !profiles.is_empty() {
            profiles.push("--new".to_string());
        }
        profiles.extend(tail.iter().cloned());
    }
    if globals.is_empty() || profiles.is_empty() {
        bail!("не удалось собрать общую стратегию");
    }
    globals.extend(profiles);
    Ok(globals)
}

pub fn parse_presets(data_dir: &Path, names: &[String]) -> Result<Vec<String>> {
    if names.is_empty() {
        bail!("не выбран ни один пресет");
    }
    let mut groups = Vec::new();
    for name in names {
        groups.push(parse_preset(data_dir, name)?);
    }
    merge_args(groups)
}

fn generated_slug(domain: &str) -> String {
    let mut slug: String = domain
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if slug.len() > 80 {
        // FNV-1a: короткий стабильный суффикс, чтобы длинные домены не
        // создавали слишком длинные имена файлов.
        let mut hash: u32 = 0x811c_9dc5;
        for byte in domain.as_bytes() {
            hash ^= u32::from(*byte);
            hash = hash.wrapping_mul(0x0100_0193);
        }
        slug.truncate(64);
        slug.push('_');
        slug.push_str(&format!("{hash:08x}"));
    }
    slug
}

fn quote_arg(arg: &str) -> Result<String> {
    if arg.contains(['\r', '\n', '"']) {
        bail!("в сгенерированной стратегии недопустимые символы");
    }
    if arg.chars().any(char::is_whitespace) {
        Ok(format!("\"{arg}\""))
    } else {
        Ok(arg.to_string())
    }
}

/// Сохраняет стратегию blockcheck2 как обычный пресет zapret2, чтобы её
/// можно было выбрать вручную или объединить с другими пресетами.
pub fn write_generated_preset(data_dir: &Path, domain: &str, args: &[String]) -> Result<String> {
    if bundle_kind(data_dir) != Some(BundleKind::V2) {
        bail!("генерация стратегий доступна только для zapret2");
    }
    let domain =
        crate::lists::validate_hostname(domain).context("некорректный домен для стратегии")?;
    if domain == "localhost" || domain.ends_with(".localhost") || domain.ends_with(".local") {
        bail!("для локального домена стратегия не нужна");
    }
    if args.is_empty() {
        bail!("пустая стратегия");
    }
    if args.iter().any(|arg| {
        !arg.starts_with("--")
            || arg.contains('\r')
            || arg.contains('\n')
            || arg.contains("exit_guard")
            || arg.contains("timer_set")
    }) {
        bail!("в сгенерированной стратегии недопустимые аргументы");
    }
    if !args.iter().any(|arg| arg.starts_with("--filter-")) {
        bail!("в сгенерированной стратегии нет фильтров");
    }
    let dir = preset_dir(data_dir)?;
    let name = format!("preset2_gen_{}.cmd", generated_slug(&domain));
    let mut content =
        String::from("@echo off\r\nstart \"zapret2: generated\" /min \"%~dp0winws2.exe\" ^\r\n");
    if !args.iter().any(|arg| arg == "--wf-tcp-empty=1") {
        content.push_str("--wf-tcp-empty=1 ^\r\n");
    }
    content.push_str("--lua-init=@\"%~dp0lua\\zapret-lib.lua\" ^\r\n");
    content.push_str("--lua-init=@\"%~dp0lua\\zapret-antidpi.lua\" ^\r\n");
    for arg in args {
        content.push_str(&quote_arg(arg)?);
        content.push(' ');
    }
    content.push_str("\r\n");
    let path = dir.join(&name);
    let tmp = path.with_extension("cmd.tmp");
    std::fs::write(&tmp, content)
        .with_context(|| format!("не удалось записать {}", tmp.display()))?;
    if path.exists() {
        let _ = std::fs::remove_file(&path);
    }
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("не удалось сохранить {}", path.display()))?;
    Ok(name)
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

    #[test]
    fn merges_multiple_presets() {
        let first = vec![
            "--wf-tcp=80,443".to_string(),
            "--lua-init=a".to_string(),
            "--filter-tcp=443".to_string(),
            "--dpi-desync=fake".to_string(),
            "--new".to_string(),
        ];
        let second = vec![
            "--wf-tcp=443,8443".to_string(),
            "--lua-init=a".to_string(),
            "--filter-udp=443".to_string(),
            "--dpi-desync=fake".to_string(),
        ];
        let args = merge_args(vec![first, second]).unwrap();
        assert_eq!(
            args.iter()
                .filter(|arg| *arg == "--wf-tcp=80,443,8443")
                .count(),
            1
        );
        assert_eq!(args.iter().filter(|arg| *arg == "--lua-init=a").count(), 1);
        assert_eq!(args.last().map(String::as_str), Some("--dpi-desync=fake"));
        assert_eq!(args.iter().filter(|arg| *arg == "--new").count(), 1);
    }

    #[test]
    fn writes_generated_preset_for_zapret2() {
        let dir = temp();
        let engine = dir.join("zapret").join("zapret-winws");
        std::fs::create_dir_all(engine.join("lua")).unwrap();
        std::fs::write(engine.join("winws2.exe"), "").unwrap();
        std::fs::write(engine.join("lua").join("zapret-lib.lua"), "").unwrap();
        std::fs::write(engine.join("lua").join("zapret-antidpi.lua"), "").unwrap();
        let args = vec![
            "--wf-tcp=443".to_string(),
            "--filter-tcp=443".to_string(),
            "--lua-desync=fake:tcp_md5".to_string(),
        ];
        let name = write_generated_preset(&dir, "Example.COM", &args).unwrap();
        assert_eq!(name, "preset2_gen_example.com.cmd");
        let parsed = parse_preset(&dir, &name).unwrap();
        assert!(parsed.iter().any(|arg| arg == "--wf-tcp-empty=1"));
        assert!(parsed.iter().any(|arg| arg.ends_with("zapret-antidpi.lua")));
        assert!(parsed.iter().any(|arg| arg == "--filter-tcp=443"));
        assert!(!parsed.iter().any(|arg| arg.contains("%~dp0")));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_unsafe_generated_args() {
        let dir = temp();
        let engine = dir.join("zapret").join("zapret-winws");
        std::fs::create_dir_all(&engine).unwrap();
        std::fs::write(engine.join("winws2.exe"), "").unwrap();
        let base = ["--filter-tcp=443".to_string()];
        assert!(write_generated_preset(&dir, "example.com", &[]).is_err());
        let mut bad = base.to_vec();
        bad.push("calc.exe".to_string());
        assert!(write_generated_preset(&dir, "example.com", &bad).is_err());
        let mut timer = base.to_vec();
        timer.push("--lua-init=timer_set('x')".to_string());
        assert!(write_generated_preset(&dir, "example.com", &timer).is_err());
        assert!(write_generated_preset(&dir, "app.localhost", &base).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
