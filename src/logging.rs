//! Логирование: stdout + manager.log (файл усекается при старте, как в legacy).
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

static LOG_FILE: OnceLock<Mutex<std::fs::File>> = OnceLock::new();

pub fn init(dir: &Path) -> anyhow::Result<()> {
    let path = dir.join("manager.log");
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)?;
    let _ = LOG_FILE.set(Mutex::new(file));
    Ok(())
}

pub fn log(message: impl AsRef<str>) {
    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let line = format!("[{now}] {}", message.as_ref());
    eprintln!("{line}");
    if let Some(file) = LOG_FILE.get() {
        if let Ok(mut f) = file.lock() {
            let _ = writeln!(f, "{line}");
            let _ = f.flush();
        }
    }
}

#[macro_export]
macro_rules! wlog {
    ($($arg:tt)*) => { $crate::logging::log(format!($($arg)*)) };
}
