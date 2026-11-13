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

/// Файл с фактическим портом панели (порт может быть выбран автоматически).
pub const PANEL_PORT_FILE: &str = "panel.port";

pub fn write_panel_port(dir: &Path, port: u16) {
    if let Ok(path) = in_dir(dir, PANEL_PORT_FILE) {
        let _ = std::fs::write(path, format!("{port}\n"));
    }
}

pub fn read_panel_port(dir: &Path) -> Option<u16> {
    let path = in_dir(dir, PANEL_PORT_FILE).ok()?;
    let text = std::fs::read_to_string(path).ok()?;
    text.trim().parse::<u16>().ok().filter(|port| *port > 0)
}

pub fn clear_panel_port(dir: &Path) {
    if let Ok(path) = in_dir(dir, PANEL_PORT_FILE) {
        let _ = std::fs::remove_file(path);
    }
}

pub fn exe_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_port_roundtrip() {
        let dir = std::env::temp_dir().join(format!("wpc-port-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(read_panel_port(&dir), None);
        write_panel_port(&dir, 8082);
        assert_eq!(read_panel_port(&dir), Some(8082));
        clear_panel_port(&dir);
        assert_eq!(read_panel_port(&dir), None);
        // мусор вместо порта не принимаем
        std::fs::write(dir.join(PANEL_PORT_FILE), "not-a-port\n").unwrap();
        assert_eq!(read_panel_port(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
