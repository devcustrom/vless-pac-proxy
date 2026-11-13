//! Внешние движки: wireproxy (WireGuard) и xray (VLESS/SS).
//! Каркас для M1: spawn/мониторинг будут перенесены сюда.
pub mod wireproxy;
pub mod xray;

use std::path::{Path, PathBuf};

pub fn bin_path(dir: &Path, base: &str) -> PathBuf {
    dir.join(crate::paths::exe_name(base))
}
