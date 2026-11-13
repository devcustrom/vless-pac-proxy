//! Установка/удаление: каталог данных, движки, автозапуск.
use std::path::Path;

use anyhow::Result;

use crate::engines::EngineKind;

pub fn install(dir: &Path) -> Result<()> {
    crate::logging::log(format!("install: каталог данных {}", dir.display()));
    crate::engines::set_data_dir(dir);
    crate::autostart::install(dir)?;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    for kind in [EngineKind::Wireproxy, EngineKind::Xray] {
        if crate::engines::find_engine(kind).is_some() {
            println!("движок {}: уже на месте", kind.label());
            continue;
        }
        println!("движок {}: скачиваю…", kind.label());
        match rt.block_on(crate::engines::downloads::ensure(dir, kind, None)) {
            Ok(path) => println!("движок {}: {}", kind.label(), path.display()),
            Err(e) => println!("движок {}: не удалось скачать — {e:#}", kind.label()),
        }
    }

    println!(
        "Папка данных: {}\nАвтозапуск включён. Запустите wireproxy-control — откроется панель\nhttp://127.0.0.1:8080/ (или нажмите на иконку в трее).",
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
