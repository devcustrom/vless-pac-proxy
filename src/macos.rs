//! macOS-специфика: установка приложения из DMG, скрытие Dock-иконки и
//! диагностика запуска из Finder (без терминала ошибки никуда не печатаются).
#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::Command;

fn bundle_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let macos = exe.parent()?;
    if macos.file_name().and_then(|v| v.to_str()) != Some("MacOS") {
        return None;
    }
    let contents = macos.parent()?;
    if contents.file_name().and_then(|v| v.to_str()) != Some("Contents") {
        return None;
    }
    let app = contents.parent()?;
    if app.extension().and_then(|v| v.to_str()) != Some("app") {
        return None;
    }
    Some(app.to_path_buf())
}

fn launched_from_image(app: &Path) -> bool {
    let path = app.to_string_lossy();
    path.starts_with("/Volumes/") || path.contains("/AppTranslocation/")
}

fn bare_launch() -> bool {
    !std::env::args_os()
        .skip(1)
        .any(|arg| !arg.to_string_lossy().starts_with("-psn_"))
}

fn writable_dir(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    let probe = path.join(format!(".wpc-write-{}", std::process::id()));
    match std::fs::write(&probe, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

fn install_dir() -> Option<PathBuf> {
    let system = PathBuf::from("/Applications");
    if writable_dir(&system) {
        return Some(system);
    }
    let home = dirs::home_dir()?;
    Some(home.join("Applications"))
}

fn app_version(app: &Path) -> Option<String> {
    let output = Command::new("plutil")
        .args(["-extract", "CFBundleShortVersionString", "raw", "-o", "-"])
        .arg(app.join("Contents/Info.plist"))
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!version.is_empty()).then_some(version)
}

fn app_running(target: &Path) -> bool {
    Command::new("pgrep")
        .arg("-f")
        .arg(target.as_os_str())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn app_binary_hash(app: &Path) -> Option<String> {
    use sha2::{Digest, Sha256};
    let data = std::fs::read(app.join("Contents/MacOS/wireproxy-control")).ok()?;
    let mut hasher = Sha256::new();
    hasher.update(&data);
    Some(hex::encode(hasher.finalize()))
}

/// Finder запустил приложение прямо из DMG (или App Translocation). Копируем
/// .app в Applications и перезапускаем уже оттуда. Это избавляет от ситуации
/// «иконка прыгнула в Dock и исчезла» и от битого автозапуска из /Volumes.
pub fn self_install_from_dmg() {
    if std::env::var_os("WPC_NO_SELF_INSTALL").is_some() || !bare_launch() {
        return;
    }
    let Some(app) = bundle_root() else {
        return;
    };
    if !launched_from_image(&app) {
        return;
    }
    let Some(dir) = install_dir() else {
        return;
    };
    let Some(name) = app.file_name() else {
        return;
    };
    let target = dir.join(name);
    if target == app {
        return;
    }
    if target.is_dir() {
        let replace = match (app_version(&app), app_version(&target)) {
            (Some(source), Some(installed)) => source != installed,
            // Одинаковая версия, но другая сборка (например, после пересборки
            // на Mac) — заменяем по хэшу бинарника.
            _ => match (app_binary_hash(&app), app_binary_hash(&target)) {
                (Some(source), Some(installed)) => source != installed,
                _ => false,
            },
        };
        if replace {
            if app_running(&target) {
                show_alert(
                    "В Applications уже запущена другая версия WireproxyControl.\n\
                     Закройте её через меню в строке состояния и запустите установку снова.",
                );
                return;
            }
            if let Err(e) = std::fs::remove_dir_all(&target) {
                show_alert(&format!(
                    "Не удалось заменить старую версию в {}: {e}",
                    target.display()
                ));
                return;
            }
        }
    }
    if !target.is_dir() {
        if let Err(e) = std::fs::create_dir_all(&dir) {
            show_alert(&format!("Не удалось создать {}: {e}", dir.display()));
            return;
        }
        let status = Command::new("ditto").arg(&app).arg(&target).status();
        match status {
            Ok(status) if status.success() => {}
            Ok(status) => {
                show_alert(&format!(
                    "Не удалось установить приложение в {} (ditto: {status}).\n\
                     Перетащите WireproxyControl в Applications вручную.",
                    dir.display()
                ));
                return;
            }
            Err(e) => {
                show_alert(&format!(
                    "Не удалось установить приложение: {e}.\n\
                     Перетащите WireproxyControl в Applications вручную."
                ));
                return;
            }
        }
        let _ = Command::new("xattr")
            .args(["-dr", "com.apple.quarantine"])
            .arg(&target)
            .status();
    }
    // Не запускаем target сразу: текущий процесс ещё держит bundle id, и
    // LaunchServices может активировать его вместо новой копии. Ждём
    // завершения текущего PID в фоне и только потом открываем .app.
    let script = format!(
        "while kill -0 {} 2>/dev/null; do sleep 0.1; done; \
         for v in /Volumes/wireproxy-control*; do \
           [ -d \"$v\" ] && hdiutil detach -force \"$v\" >/dev/null 2>&1 || true; \
         done; open \"$1\"",
        std::process::id()
    );
    let spawned = Command::new("/bin/sh")
        .args(["-c", &script, "wpc-install"])
        .arg(&target)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    match spawned {
        Ok(_) => std::process::exit(0),
        Err(e) => show_alert(&format!(
            "Приложение установлено в {}, но не запустилось: {e}",
            target.display()
        )),
    }
}

/// Установленное приложение при старте убирает оставшиеся примонтированные
/// DMG wireproxy-control. Если мы сами запущены из такого тома — не трогаем:
/// self_install_from_dmg извлечёт его после выхода.
pub fn eject_leftover_install_volumes() {
    if let Some(app) = bundle_root() {
        if launched_from_image(&app) {
            return;
        }
    }
    let script = "for v in /Volumes/wireproxy-control*; do \
                    [ -d \"$v\" ] && hdiutil detach -force \"$v\" >/dev/null 2>&1 || true; \
                  done";
    let _ = Command::new("/bin/sh")
        .args(["-c", script])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

fn is_hidden_launch() -> bool {
    std::env::args_os().any(|arg| arg.to_string_lossy() == "--hidden")
}

pub fn show_alert(message: &str) {
    if is_hidden_launch() {
        return;
    }
    let message: String = message
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace(['\n', '\r'], " ")
        .chars()
        .take(800)
        .collect();
    let script = format!("display alert \"wireproxy-control\" message \"{message}\" as critical");
    let _ = Command::new("osascript").args(["-e", &script]).status();
}

pub fn show_error(error: &anyhow::Error) {
    show_alert(&format!("{error:#}"));
}

pub fn init_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let text = format!("{info}");
        let _ = std::fs::write("/tmp/wireproxy-control-panic.log", &text);
        show_alert(&format!("Критическая ошибка: {text}"));
        previous(info);
    }));
}
