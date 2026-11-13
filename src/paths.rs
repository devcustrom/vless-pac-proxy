//! Каталог данных и защита путей.
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

/// Portable-режим: если рядом с бинарником лежат данные (wg.conf/settings.json/
/// wireproxy.conf), используем эту папку. Иначе — системный каталог приложения.
pub fn resolve_data_dir(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(p) = explicit {
        return Ok(p);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if dir.join("wg.conf").exists()
                || dir.join("settings.json").exists()
                || dir.join("wireproxy.conf").exists()
            {
                return Ok(dir.to_path_buf());
            }
        }
    }
    let base = dirs::data_local_dir()
        .or_else(dirs::home_dir)
        .context("не удалось определить каталог данных")?;
    Ok(base.join("wireproxy-control"))
}

pub fn ensure_layout(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("не удалось создать каталог данных {}", dir.display()))?;
    std::fs::create_dir_all(dir.join("certs"))?;
    Ok(())
}

/// Путь к файлу состояния: только basename внутри каталога данных.
pub fn in_dir(dir: &Path, name: &str) -> Result<PathBuf> {
    if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
        bail!("некорректное имя файла состояния: {name}");
    }
    let candidate = dir.join(name);
    let root = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let parent = candidate
        .parent()
        .map(|p| p.canonicalize().unwrap_or_else(|_| p.to_path_buf()))
        .unwrap_or_else(|| root.clone());
    if !parent.starts_with(&root) {
        bail!("путь вне каталога данных");
    }
    Ok(candidate)
}

pub fn exe_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}
