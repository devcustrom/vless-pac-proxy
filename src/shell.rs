//! Системные действия: открыть URL в браузере, запустить скрипт с повышением
//! прав. Windows — ShellExecuteW (без парсинга командной строки), macOS — open.
use std::path::Path;

use anyhow::Result;

pub fn open_url(url: &str) -> Result<()> {
    imp::open_url(url)
}

pub fn run_elevated(path: &Path) -> Result<()> {
    imp::run_elevated(path)
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

    fn shell_execute(verb: &str, target: &str) -> Result<()> {
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
                1,
            )
        };
        if rc as isize <= 32 {
            anyhow::bail!("ShellExecuteW вернул {}", rc as isize);
        }
        Ok(())
    }

    pub fn open_url(url: &str) -> Result<()> {
        shell_execute("open", url)
    }

    pub fn run_elevated(path: &Path) -> Result<()> {
        shell_execute("runas", &path.to_string_lossy())
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

    pub fn run_elevated(path: &Path) -> Result<()> {
        std::process::Command::new("open")
            .arg(path)
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

    pub fn run_elevated(path: &Path) -> Result<()> {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(Into::into)
    }
}
