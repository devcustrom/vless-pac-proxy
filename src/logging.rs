//! Логирование: stdout + manager.log (дописываем; `reset` усекает при старте
//! менеджера, после того как экземпляр подтвердил единственность).
use std::fs::OpenOptions;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

static LOG_FILE: OnceLock<Mutex<std::fs::File>> = OnceLock::new();

pub fn init(dir: &Path) -> anyhow::Result<()> {
    let path = dir.join("manager.log");
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&path)?;
    let _ = LOG_FILE.set(Mutex::new(file));
    Ok(())
}

/// Усекает лог: вызывается только владельцем single-instance.
pub fn reset() {
    if let Some(file) = LOG_FILE.get() {
        if let Ok(mut f) = file.lock() {
            let _ = f.set_len(0);
            let _ = f.flush();
        }
    }
}

pub fn log(message: impl AsRef<str>) {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let line = format!("[{now}] {}", message.as_ref());
    eprintln!("{line}");
    if let Some(file) = LOG_FILE.get() {
        if let Ok(mut f) = file.lock() {
            let _ = f.seek(SeekFrom::End(0));
            let _ = writeln!(f, "{line}");
            let _ = f.flush();
        }
    }
}

#[macro_export]
macro_rules! wlog {
    ($($arg:tt)*) => { $crate::logging::log(format!($($arg)*)) };
}
