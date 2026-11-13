//! wireproxy-control — умный прокси-стек: WireGuard (wireproxy) + VLESS/SS (xray),
//! веб-панель, умный прокси по доменам, локальные домены с HTTPS, трей.
//!
//! Подкоманды:
//!   (без аргументов)   — менеджер + панель + трей
//!   install            — установка: папка данных, движки, автозапуск, панель
//!   uninstall          — удаление автозапуска (данные остаются)
//!   trust-certs        — установить локальный CA в доверенные
//!   mcp                — MCP-сервер (stdio JSON-RPC)
//!   version            — версия
#![allow(dead_code)] // временно на время переноса (M0→M6): модули подключаются поэтапно
use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod api;
mod engines;
mod install;
mod lists;
mod local_tls;
mod logging;
mod mcp;
mod net;
mod paths;
mod routing;
mod settings;
mod state;
mod systemproxy;
mod vless;

#[derive(Parser, Debug)]
#[command(
    name = "wireproxy-control",
    version,
    about = "Умный прокси-стек WireGuard + VLESS/SS"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Каталог данных (по умолчанию: рядом с exe в portable-режиме, иначе LocalAppData)
    #[arg(long, global = true, env = "WPC_DATA_DIR")]
    data_dir: Option<PathBuf>,

    /// Запуск без окна консоли (автозапуск)
    #[arg(long, global = true, hide = true)]
    hidden: bool,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Запустить менеджер, панель и трей (режим по умолчанию)
    #[command(hide = true)]
    Run,
    /// Установка: папка данных, движки wireproxy/xray, автозапуск, открыть панель
    Install,
    /// Удалить автозапуск (данные и сертификаты остаются)
    Uninstall,
    /// Установить локальный CA в доверенные (запросит права администратора)
    TrustCerts,
    /// MCP-сервер для агентов (stdio JSON-RPC)
    Mcp,
    /// Показать версию
    Version,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let dir = paths::resolve_data_dir(cli.data_dir.clone())?;
    paths::ensure_layout(&dir)?;
    logging::init(&dir)?;

    match cli.command.unwrap_or(Command::Run) {
        Command::Run => {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()?;
            rt.block_on(api::run(dir))
        }
        Command::Install => install::install(&dir),
        Command::Uninstall => install::uninstall(&dir),
        Command::TrustCerts => local_tls::trust_certs(&dir),
        Command::Mcp => mcp::run_stdio(&dir),
        Command::Version => {
            println!("wireproxy-control {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    }
}
