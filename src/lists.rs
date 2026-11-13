//! Списки доменов (itdog/refilter) — перенос в M1.
use std::path::Path;

use anyhow::Result;

pub const MODE: &str = "blocked-only";

pub async fn refresh(dir: &Path) -> Result<(usize, usize)> {
    let _ = dir;
    anyhow::bail!("списки доменов ещё не перенесены (этап M1)")
}
