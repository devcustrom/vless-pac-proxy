//! Локальный CA и сертификаты локальных доменов (rcgen) — этап M3.
use std::path::Path;

use anyhow::Result;

pub fn trust_certs(dir: &Path) -> Result<()> {
    let ca = dir.join("certs").join("ca.crt");
    if !ca.exists() {
        anyhow::bail!(
            "CA ещё не создан — запустите приложение один раз (ожидался {})",
            ca.display()
        );
    }
    println!(
        "Установка CA в доверенные (UAC/админ) — этап M3. Файл CA: {}",
        ca.display()
    );
    Ok(())
}
