//! xray: spawn/monitor (M1). Пока каркас.
#![allow(dead_code)]
use std::path::Path;

pub fn has_config(dir: &Path) -> bool {
    dir.join("xray-config.json").exists()
}
