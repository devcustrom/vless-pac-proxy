//! Автозапуск при входе в систему.
//! Windows: HKCU\Software\Microsoft\Windows\CurrentVersion\Run.
//! macOS: ~/Library/LaunchAgents/com.wireproxy-control.plist (загрузку
//! выполняет launchd; установщик M5 покажет команду для немедленного старта).
use std::path::Path;

use anyhow::Result;

pub fn install(dir: &Path) -> Result<()> {
    imp::install(dir)
}

pub fn uninstall() -> Result<()> {
    imp::uninstall()
}

pub fn installed() -> bool {
    imp::installed()
}

/// Если автозапуск включён, он должен указывать на текущий бинарник.
/// Иначе после переезда/обновления из другого места при загрузке остаётся
/// старая копия приложения.
pub fn ensure_current(dir: &Path) -> Result<()> {
    imp::ensure_current(dir)
}

#[cfg(windows)]
mod imp {
    use anyhow::Result;
    use std::path::Path;
    use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE};
    use winreg::RegKey;

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE: &str = "wireproxy-control";

    pub fn install(dir: &Path) -> Result<()> {
        let exe = std::env::current_exe()?;
        let cmd = format!(
            "\"{}\" --hidden --data-dir \"{}\"",
            exe.display(),
            dir.display()
        );
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = hkcu.open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)?;
        key.set_value(VALUE, &cmd)?;
        println!("Автозапуск включён (HKCU\\Run): {cmd}");
        Ok(())
    }

    pub fn uninstall() -> Result<()> {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = hkcu.open_subkey_with_flags(RUN_KEY, KEY_SET_VALUE)?;
        match key.delete_value(VALUE) {
            Ok(()) => println!("Автозапуск выключен"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                println!("Автозапуск и так не был включён")
            }
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }

    pub fn installed() -> bool {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        hkcu.open_subkey_with_flags(RUN_KEY, KEY_READ)
            .and_then(|key| key.get_value::<String, _>(VALUE))
            .is_ok()
    }

    pub fn ensure_current(dir: &Path) -> Result<()> {
        let exe = std::env::current_exe()?;
        let expected = format!(
            "\"{}\" --hidden --data-dir \"{}\"",
            exe.display(),
            dir.display()
        );
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let key = hkcu.open_subkey_with_flags(RUN_KEY, KEY_READ)?;
        match key.get_value::<String, _>(VALUE) {
            Ok(value) if value == expected => Ok(()),
            Ok(_) => install(dir),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use anyhow::{Context, Result};
    use std::path::{Path, PathBuf};

    const LABEL: &str = "com.wireproxy-control";

    fn plist_path() -> Result<PathBuf> {
        let home = dirs::home_dir().context("не найден домашний каталог")?;
        Ok(home
            .join("Library")
            .join("LaunchAgents")
            .join(format!("{LABEL}.plist")))
    }

    fn xml_escape(value: &str) -> String {
        value
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }

    pub fn install(dir: &Path) -> Result<()> {
        let exe = std::env::current_exe()?;
        let path = plist_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let exe = xml_escape(&exe.to_string_lossy());
        let dir_raw = dir.to_string_lossy();
        let dir = xml_escape(&dir_raw);
        let out_log = xml_escape(&format!("{}/manager.log", dir_raw));
        let plist = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\">\n\
             <dict>\n\
               <key>Label</key><string>{LABEL}</string>\n\
               <key>ProgramArguments</key>\n\
               <array><string>{exe}</string><string>--hidden</string><string>--data-dir</string><string>{dir}</string></array>\n\
               <key>RunAtLoad</key><true/>\n\
               <key>KeepAlive</key>\n\
               <dict><key>SuccessfulExit</key><false/></dict>\n\
               <key>StandardOutPath</key><string>{out_log}</string>\n\
               <key>StandardErrorPath</key><string>{out_log}</string>\n\
             </dict>\n\
             </plist>\n"
        );
        std::fs::write(&path, plist)?;
        println!(
            "Автозапуск записан: {}\nЧтобы включить сейчас: launchctl bootstrap gui/$(id -u) \"{}\"",
            path.display(),
            path.display()
        );
        Ok(())
    }

    pub fn uninstall() -> Result<()> {
        let path = plist_path()?;
        match std::fs::remove_file(&path) {
            Ok(()) => println!(
                "Автозапуск выключен (удалён {}); текущий процесс продолжит работать",
                path.display()
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                println!("Автозапуск и так не был включён")
            }
            Err(e) => return Err(e.into()),
        }
        // Не делаем launchctl bootout из собственного процесса: если он
        // запущен этим LaunchAgent'ом, SIGTERM убьёт менеджер до ответа и
        // cleanup. Плитки достаточно — со следующего входа агент не поднимется.
        Ok(())
    }

    pub fn installed() -> bool {
        plist_path().map(|p| p.is_file()).unwrap_or(false)
    }

    pub fn ensure_current(dir: &Path) -> Result<()> {
        let path = plist_path()?;
        if !path.is_file() {
            return Ok(());
        }
        let exe = xml_escape(&std::env::current_exe()?.to_string_lossy());
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        if text.contains(&exe) {
            return Ok(());
        }
        install(dir)
    }
}
