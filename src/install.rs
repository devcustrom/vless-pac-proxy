//! Установка/удаление: автозапуск уже здесь, загрузчик движков и мастер
//! первого запуска — этап M5.
use std::path::Path;

use anyhow::Result;

pub fn install(dir: &Path) -> Result<()> {
    crate::logging::log(format!("install: каталог данных {}", dir.display()));
    crate::autostart::install(dir)?;
    println!(
        "Папка данных: {}\nАвтозапуск включён. Запустите wireproxy-control — панель откроется\nпо адресу http://127.0.0.1:8080/ (движки скачиваются при первом запуске).",
        dir.display()
    );
    Ok(())
}

pub fn uninstall(dir: &Path) -> Result<()> {
    crate::logging::log(format!("uninstall: {}", dir.display()));
    crate::autostart::uninstall()?;
    println!("Данные в {} не тронуты.", dir.display());
    Ok(())
}
