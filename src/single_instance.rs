//! Один экземпляр на каталог данных: эксклюзивная блокировка `instance.lock`.
//! Блокировка снимается автоматически при завершении процесса.
use std::fs::{File, OpenOptions};
use std::path::Path;

use anyhow::{Context, Result};

pub struct Instance {
    _file: File,
}

/// Возвращает `None`, если другой экземпляр уже держит блокировку.
pub fn acquire(dir: &Path) -> Result<Option<Instance>> {
    let path = dir.join("instance.lock");
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&path)
        .with_context(|| format!("не удалось открыть {}", path.display()))?;
    match fs2::FileExt::try_lock_exclusive(&file) {
        Ok(()) => Ok(Some(Instance { _file: file })),
        Err(_) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_lock_is_refused() {
        let dir = std::env::temp_dir().join(format!("wpc-lock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let first = acquire(&dir).unwrap();
        assert!(first.is_some());
        assert!(acquire(&dir).unwrap().is_none());
        drop(first);
        assert!(acquire(&dir).unwrap().is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
