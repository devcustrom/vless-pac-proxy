//! wireproxy-control — умный прокси-стек: WireGuard (wireproxy) + VLESS/SS (xray),
//! веб-панель, умный прокси по доменам, локальные домены с HTTPS, трей.
//!
//! Подкоманды:
//!   (без аргументов)   — менеджер + панель + трей
//!   install            — установка: автозапуск, папка данных
//!   uninstall          — удаление автозапуска (данные остаются)
//!   stop               — остановить запущенный менеджер
//!   trust-certs        — установить локальный CA в доверенные
//!   sync-hosts         — подготовить скрипт синхронизации hosts
//!   mcp                — MCP-сервер (stdio JSON-RPC)
//!   version            — версия
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
#![recursion_limit = "256"]
#![allow(dead_code)] // временно на время переноса (M0→M6): модули подключаются поэтапно
use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod api;
mod autostart;
mod backup;
mod engines;
mod install;
mod lists;
mod local_hosts;
mod local_tls;
mod logging;
#[cfg(target_os = "macos")]
mod macos;
mod mcp;
mod net;
mod paths;
mod routing;
mod settings;
mod shell;
mod single_instance;
mod site_check;
mod state;
mod store;
mod systemproxy;
mod telegram;
#[cfg(any(windows, target_os = "macos"))]
mod tray;
mod trust_status;
mod update;
mod vless;
mod zapret;

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

    /// Порт панели (по умолчанию 8080; полезно, если порт занят)
    #[arg(long, global = true, env = "WPC_PANEL_PORT", default_value_t = 8080)]
    panel_port: u16,

    /// Запуск без окна консоли (автозапуск)
    #[arg(long, global = true, hide = true)]
    hidden: bool,

    /// Запуск без трея (отладка и сервисный режим)
    #[arg(long, global = true, hide = true)]
    no_tray: bool,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Запустить менеджер, панель и трей (режим по умолчанию)
    #[command(hide = true)]
    Run,
    /// Установка: папка данных, автозапуск, открыть панель
    Install,
    /// Удалить автозапуск (данные и сертификаты остаются)
    Uninstall,
    /// Остановить запущенный менеджер (выключит системный прокси)
    Stop,
    /// Установить локальный CA в доверенные (запросит права администратора)
    TrustCerts,
    /// Подготовить скрипт синхронизации hosts для локальных доменов
    SyncHosts,
    /// MCP-сервер для агентов (stdio JSON-RPC)
    Mcp,
    /// Служебный шаг обновления: заменить бинарник и перезапустить менеджер
    #[command(hide = true)]
    ApplyUpdate {
        #[arg(long)]
        pid: u32,
        #[arg(long)]
        target: PathBuf,
    },
    /// Служебный шаг восстановления бэкапа после выхода менеджера
    #[command(hide = true)]
    Restore {
        #[arg(long)]
        pid: u32,
        #[arg(long)]
        zip: PathBuf,
    },
    /// Служебный шаг проверки стратегий каталога (запускается с UAC)
    #[command(hide = true)]
    CatalogTest,
    /// Показать версию
    Version,
}

fn clean_cli_args(args: impl IntoIterator<Item = std::ffi::OsString>) -> Vec<std::ffi::OsString> {
    // Finder на macOS передаёт -psn_0_…; clap на нём падал бы до логирования,
    // и приложение выглядело бы как «ничего не запустилось».
    args.into_iter()
        .filter(|arg| !arg.to_string_lossy().starts_with("-psn_"))
        .collect()
}

fn main() {
    #[cfg(target_os = "macos")]
    macos::init_panic_hook();
    if let Err(error) = run() {
        eprintln!("{error:#}");
        #[cfg(target_os = "macos")]
        macos::show_error(&error);
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    #[cfg(all(windows, not(debug_assertions)))]
    attach_console();

    // Finder может запустить .app прямо из DMG. Устанавливаем в Applications
    // и перезапускаемся оттуда до создания каталога данных и tray.
    #[cfg(target_os = "macos")]
    {
        macos::self_install_from_dmg();
        macos::eject_leftover_install_volumes();
    }

    let cli = Cli::parse_from(clean_cli_args(std::env::args_os()));
    let dir = paths::resolve_data_dir(cli.data_dir.clone())?;
    paths::ensure_layout(&dir)?;
    logging::init(&dir)?;

    match cli.command.unwrap_or(Command::Run) {
        Command::Run => run_manager(dir, cli.panel_port, cli.no_tray, cli.hidden),
        Command::Install => install::install(&dir),
        Command::Uninstall => install::uninstall(&dir),
        Command::Stop => stop_manager(&dir, cli.panel_port),
        Command::TrustCerts => {
            local_tls::trust_certs(&dir)?;
            shell::run_elevated(&dir.join(local_tls::trust_script_name()))
        }
        Command::SyncHosts => local_hosts::sync_hosts(&dir),
        Command::Mcp => mcp::run_stdio(&dir),
        Command::ApplyUpdate { pid, target } => {
            update::apply_pending_update(pid, &target, &dir, cli.panel_port)
        }
        Command::Restore { pid, zip } => backup::apply_restore(pid, &zip, &dir, cli.panel_port),
        Command::CatalogTest => zapret::catalog::run_test_helper(&dir),
        Command::Version => {
            println!("wireproxy-control {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
    }
}

fn run_manager(dir: PathBuf, panel_port: u16, no_tray: bool, hidden: bool) -> anyhow::Result<()> {
    let dir = std::path::absolute(&dir).unwrap_or(dir);
    // Закрытие терминала не должно убивать менеджер и его трей: sidecar и
    // движки останавливаются только через штатный cleanup/LaunchAgent.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
    }
    let requested_port = panel_port;
    let panel_port = pick_panel_port(requested_port);
    let _instance = match single_instance::acquire(&dir)? {
        Some(instance) => instance,
        None => {
            let existing_port = paths::read_panel_port(&dir).unwrap_or(panel_port);
            println!(
                "уже запущен другой экземпляр (данные: {}); панель: http://127.0.0.1:{existing_port}/",
                dir.display()
            );
            #[cfg(target_os = "macos")]
            let reachable = std::net::TcpStream::connect_timeout(
                &std::net::SocketAddr::from(([127, 0, 0, 1], existing_port)),
                std::time::Duration::from_millis(500),
            )
            .is_ok();
            if !hidden {
                let _ = shell::open_url(&format!("http://127.0.0.1:{existing_port}/"));
            }
            #[cfg(target_os = "macos")]
            if !reachable {
                crate::macos::show_alert(&format!(
                    "wireproxy-control уже запущен, но панель не отвечает.\n\
                     Завершите старый процесс wireproxy-control в «Мониторинге системы» \
                     или перезагрузите Mac и запустите приложение снова."
                ));
            }
            return Ok(());
        }
    };
    logging::reset();
    if panel_port != requested_port {
        wlog!("порт {requested_port} занят — панель на {panel_port}");
    }
    // Движки запускаются относительным именем (.\wireproxy.exe): Windows
    // разрешает его от cwd родителя, поэтому переходим в каталог данных.
    std::env::set_current_dir(&dir).map_err(|e| {
        anyhow::anyhow!("не удалось перейти в каталог данных {}: {e}", dir.display())
    })?;
    crate::vless::pool::cleanup_test_processes();
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    // На macOS после установки приложение живёт только в menu bar. Панель
    // автоматически открываем лишь при первом запуске (нет settings.json),
    // дальше — по клику в трее. Windows-поведение не меняем.
    let first_run = !dir.join("settings.json").exists();
    let interactive = if cfg!(target_os = "macos") {
        !hidden && first_run
    } else {
        !hidden
    };
    let st = {
        let _guard = rt.enter();
        state::AppState::new(dir, panel_port, interactive)
    };
    if autostart::installed()
        && !cfg!(debug_assertions)
        && std::env::var_os("WPC_TEST_MODE").is_none()
    {
        if let Err(e) = autostart::ensure_current(&st.dir) {
            wlog!("автозапуск: не удалось обновить путь — {e:#}");
        }
    }

    #[cfg(any(windows, target_os = "macos"))]
    {
        if !no_tray {
            let handle = rt.handle().clone();
            let tray_state = st.clone();
            std::thread::spawn(move || {
                if let Err(e) = rt.block_on(api::run(st)) {
                    wlog!("менеджер остановлен: {e:#}");
                }
            });
            return match tray::run(tray_state, handle) {
                Ok(()) => Ok(()),
                Err(e) => {
                    wlog!("трей недоступен: {e:#}; продолжаем без трея");
                    #[cfg(target_os = "macos")]
                    crate::macos::show_alert(&format!(
                        "Не удалось создать иконку в menu bar: {e:#}"
                    ));
                    loop {
                        std::thread::park_timeout(std::time::Duration::from_secs(3600));
                    }
                }
            };
        }
    }

    let _ = no_tray;
    rt.block_on(api::run(st))
}

/// Первый свободный порт панели, начиная с preferred (8081 занят умным
/// прокси). Освобождаем проверочный listener сразу — гонка маловероятна.
fn pick_panel_port(preferred: u16) -> u16 {
    for port in preferred..=preferred.saturating_add(20) {
        if port == 8081 {
            continue;
        }
        if std::net::TcpListener::bind(("127.0.0.1", port)).is_ok() {
            return port;
        }
    }
    preferred
}

/// Просит запущенный менеджер завершиться через локальный API (с X-WPC).
fn stop_manager(dir: &std::path::Path, panel_port: u16) -> anyhow::Result<()> {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    let mut ports: Vec<u16> = Vec::new();
    if let Some(saved) = paths::read_panel_port(dir) {
        ports.push(saved);
    }
    if !ports.contains(&panel_port) {
        ports.push(panel_port);
    }

    let mut wrong_server = None;
    for port in &ports {
        let addr = format!("127.0.0.1:{port}");
        let Ok(mut sock) = TcpStream::connect(&addr) else {
            continue;
        };
        sock.set_read_timeout(Some(Duration::from_secs(5)))?;
        let request = format!(
            "POST /api/quit HTTP/1.1\r\nHost: {addr}\r\nX-WPC: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        );
        sock.write_all(request.as_bytes())?;
        let mut response = String::new();
        let _ = sock.read_to_string(&mut response);
        let status_ok =
            response.starts_with("HTTP/1.1 200") || response.starts_with("HTTP/1.0 200");
        if status_ok && response.contains("\"ok\":true") {
            println!("менеджер останавливается (порт {port})");
            return Ok(());
        }
        wrong_server = Some(response.lines().next().unwrap_or("нет ответа").to_string());
    }
    match wrong_server {
        Some(line) => println!("не удалось остановить: {line}"),
        None => println!("менеджер не запущен (искали порты: {ports:?})"),
    }
    Ok(())
}

/// Release-сборка на Windows — GUI-subsystem: подключаемся к консоли
/// родителя, чтобы `install`/`mcp`/`--help` печатали вывод как обычно.
#[cfg(all(windows, not(debug_assertions)))]
fn attach_console() {
    use windows_sys::Win32::System::Console::{
        AttachConsole, SetConsoleCP, SetConsoleOutputCP, ATTACH_PARENT_PROCESS,
    };
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
        // русский вывод в cmd/PowerShell: переключаем консоль на UTF-8
        SetConsoleOutputCP(65001);
        SetConsoleCP(65001);
    }
}

#[cfg(test)]
mod tests {
    use super::clean_cli_args;
    use std::ffi::OsString;

    #[test]
    fn strips_macos_process_serial_number() {
        let args = vec![
            OsString::from("/Applications/WireproxyControl.app/Contents/MacOS/wireproxy-control"),
            OsString::from("-psn_0_123456"),
            OsString::from("--hidden"),
        ];
        let cleaned = clean_cli_args(args);
        assert_eq!(cleaned.len(), 2);
        assert_eq!(cleaned[1], OsString::from("--hidden"));
    }
}
