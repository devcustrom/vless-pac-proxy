//! Пул конфигов из подписок (переносится в M2, каркас для компиляции).
use std::path::Path;

use anyhow::Result;

pub async fn refresh_pool(dir: &Path) -> Result<()> {
    let _ = dir;
    anyhow::bail!("пул подписок ещё не перенесён (этап M2)")
}
