//! Shared mobile core for the standalone Android app.
//!
//! Desktop keeps its process supervisors and UI. This crate only reuses the
//! pure parsing/config-building parts and will be exported to Kotlin through
//! FFI later.

use std::path::{Path, PathBuf};

/// The desktop link parser only needs `in_dir` for the `save_*` helpers. Mobile
/// keeps profiles in Room, so the path helper is intentionally minimal.
mod paths {
    use super::{Path, PathBuf};

    pub fn in_dir(dir: &Path, name: &str) -> anyhow::Result<PathBuf> {
        if name.is_empty()
            || name == "."
            || name == ".."
            || name.contains('/')
            || name.contains('\\')
        {
            anyhow::bail!("invalid data file name");
        }
        Ok(dir.join(name))
    }
}

pub mod vless;

pub mod mobile_config;
pub mod subscription;
pub mod wireguard;

pub use vless::link::{
    build_outbound, config_for_parsed, generate_config, parse_any, parse_ss, parse_trojan,
    parse_vless, parse_vmess, LinkInfo, ParsedLink,
};
