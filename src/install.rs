//! Установка/удаление (полный сценарий — этап M5).
use std::path::Path;

use anyhow::Result;

pub fn install(dir: &Path) -> Result<()> {
    crate::logging::log(format!("install: каталог данных {}", dir.display()));
    println!(
        "Папка данных: {}\nАвтозапуск, загрузка движков и открытие панели появятся в этапе M5.",
        dir.display()
    );
    Ok(())
}

pub fn uninstall(dir: &Path) -> Result<()> {
    crate::logging::log(format!("uninstall: {}", dir.display()));
    println!("Удаление автозапуска — этап M5. Данные в {} не тронуты.", dir.display());
    Ok(())
}
