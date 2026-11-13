//! Умный HTTP-прокси 127.0.0.1:8081 (переносится в M1).
use std::path::Path;

use anyhow::Result;

pub async fn serve(dir: &Path) -> Result<()> {
    let _ = dir;
    anyhow::bail!("умный прокси ещё не перенесён (этап M1)")
}
