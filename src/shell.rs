//! Системные действия: открыть URL в браузере, запустить скрипт с повышением
//! прав. Windows — ShellExecuteW (без парсинга командной строки), macOS —
//! osascript с нативным диалогом пароля.
use std::path::Path;

use anyhow::Result;

pub fn open_url(url: &str) -> Result<()> {
    imp::open_url(url)
}

/// Неинтерактивный скрипт: UAC/пароль есть, окна терминала нет.
pub fn run_elevated(path: &Path) -> Result<()> {
    imp::run_elevated(path, false)
}

/// Интерактивный скрипт (например, редактор hosts) — окно нужно показать.
pub fn run_elevated_interactive(path: &Path) -> Result<()> {
    imp::run_elevated(path, true)
}

#[cfg(windows)]
mod imp {
    use anyhow::Result;
    use std::path::Path;

    fn wide(value: &str) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        std::ffi::OsStr::new(value)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn shell_execute(verb: &str, target: &str, show: i32) -> Result<()> {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        let verb = wide(verb);
        let target = wide(target);
        let rc = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                show,
            )
        };
        if rc as isize <= 32 {
            anyhow::bail!("ShellExecuteW вернул {}", rc as isize);
        }
        Ok(())
    }

    pub fn open_url(url: &str) -> Result<()> {
        shell_execute("open", url, 1)
    }

    pub fn run_elevated(path: &Path, visible: bool) -> Result<()> {
        // SW_HIDE для установщиков: UAC остаётся, консоль не мелькает.
        shell_execute(
            "runas",
            &path.to_string_lossy(),
            if visible { 1 } else { 0 },
        )
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use anyhow::Result;
    use std::path::Path;

    pub fn open_url(url: &str) -> Result<()> {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(Into::into)
    }

    pub fn run_elevated(path: &Path, visible: bool) -> Result<()> {
        if visible {
            // Интерактивный сценарий (nano и т.п.) требует Terminal.
            return std::process::Command::new("open")
                .arg(path)
                .spawn()
                .map(|_| ())
                .map_err(Into::into);
        }
        // Нативный диалог пароля macOS вместо окна Terminal.
        let escaped = path
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        let script =
            format!("do shell script \"/bin/sh \" & quoted form of \"{escaped}\" with administrator privileges");
        std::process::Command::new("osascript")
            .args(["-e", &script])
            .spawn()
            .map(|_| ())
            .map_err(Into::into)
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
mod imp {
    use anyhow::Result;
    use std::path::Path;

    pub fn open_url(url: &str) -> Result<()> {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(Into::into)
    }

    pub fn run_elevated(path: &Path, _visible: bool) -> Result<()> {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(Into::into)
    }
}
