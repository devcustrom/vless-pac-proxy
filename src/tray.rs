//! Трей Windows/macOS: открыть панель (panel.localhost при доверенном CA),
//! установить сертификаты, обновить списки, перезапустить движки, выход
//! с очисткой (системный прокси выключается, движки останавливаются).
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use winit::application::ApplicationHandler;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};

use crate::state::AppState;
use crate::wlog;

pub struct Ctx {
    pub state: Arc<AppState>,
    pub handle: tokio::runtime::Handle,
}

struct App {
    ctx: Ctx,
    tray: Option<TrayIcon>,
}

pub fn run(state: Arc<AppState>, handle: tokio::runtime::Handle) -> Result<()> {
    let event_loop = EventLoop::new()?;
    let mut app = App {
        ctx: Ctx { state, handle },
        tray: None,
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}

impl ApplicationHandler for App {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
        if self.tray.is_none() {
            match build_tray() {
                Ok(tray) => {
                    self.tray = Some(tray);
                    wlog!("трей: меню готово");
                }
                Err(e) => wlog!("трей: не создан — {e:#}"),
            }
        }
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: winit::window::WindowId,
        _event: winit::event::WindowEvent,
    ) {
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            self.on_menu(event.id().0.as_str());
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(200),
        ));
    }
}

fn build_tray() -> Result<TrayIcon> {
    let menu = Menu::with_items(&[
        &MenuItem::with_id("open", "Открыть панель", true, None),
        &MenuItem::with_id("certs", "Установить сертификаты…", true, None),
        &PredefinedMenuItem::separator(),
        &MenuItem::with_id("refresh", "Обновить списки доменов", true, None),
        &MenuItem::with_id("restart", "Перезапустить движки", true, None),
        &PredefinedMenuItem::separator(),
        &MenuItem::with_id("exit", "Выход", true, None),
    ])?;
    let icon = Icon::from_rgba(icon_rgba(), 32, 32)?;
    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("wireproxy-control — умный прокси")
        .with_icon(icon)
        .build()?;
    Ok(tray)
}

fn icon_rgba() -> Vec<u8> {
    let size = 32u32;
    let mut data = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - 15.5;
            let dy = y as f32 - 15.5;
            let dist = (dx * dx + dy * dy).sqrt();
            let idx = ((y * size + x) * 4) as usize;
            if dist < 15.0 {
                data[idx] = 24;
                data[idx + 1] = 24;
                data[idx + 2] = 27;
                data[idx + 3] = 255;
                if (11.5..13.5).contains(&dist) {
                    data[idx] = 34;
                    data[idx + 1] = 197;
                    data[idx + 2] = 94;
                }
            }
        }
    }
    data
}

impl App {
    fn on_menu(&self, id: &str) {
        match id {
            "open" => {
                let url = panel_url(&self.ctx);
                if let Err(e) = open_url(&url) {
                    wlog!("трей: открыть панель — {e:#}");
                }
            }
            "certs" => match crate::local_tls::trust_certs(&self.ctx.state.dir) {
                Ok(()) => {
                    let script = self
                        .ctx
                        .state
                        .dir
                        .join(crate::local_tls::trust_script_name());
                    if let Err(e) = run_elevated(&script) {
                        wlog!("трей: сертификаты — {e:#}");
                    }
                }
                Err(e) => wlog!("трей: сертификаты — {e:#}"),
            },
            "refresh" => {
                let st = self.ctx.state.clone();
                self.ctx.handle.spawn(async move {
                    if let Err(e) = crate::lists::refresh(&st).await {
                        wlog!("трей: списки — {e:#}");
                    }
                });
            }
            "restart" => {
                self.ctx.state.wg.restart();
                self.ctx.state.xr.restart();
                wlog!("трей: движки перезапускаются");
            }
            "exit" => {
                wlog!("трей: выход");
                crate::api::cleanup(&self.ctx.state);
                std::thread::sleep(Duration::from_millis(600));
                std::process::exit(0);
            }
            _ => {}
        }
    }
}

/// Панель через panel.localhost, если домен привязан к панели и CA доверен,
/// иначе loopback-адрес.
fn panel_url(ctx: &Ctx) -> String {
    let settings = ctx.state.settings_snapshot();
    let mapped = ctx
        .state
        .local_domains
        .lock()
        .map(|domains| {
            domains
                .iter()
                .any(|d| d.domain == "panel.localhost" && d.port == ctx.state.panel_port)
        })
        .unwrap_or(false);
    if mapped && crate::trust_status::is_trusted(&ctx.state.dir) {
        let port = settings.local_https_port;
        if port == 443 {
            "https://panel.localhost/".to_string()
        } else {
            format!("https://panel.localhost:{port}/")
        }
    } else {
        format!("http://127.0.0.1:{}/", ctx.state.panel_port)
    }
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(windows)]
fn shell_execute(verb: &str, target: &str) -> Result<()> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    let verb = wide(verb);
    let target = wide(target);
    let rc = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        )
    };
    if rc as isize <= 32 {
        anyhow::bail!("ShellExecuteW вернул {}", rc as isize);
    }
    Ok(())
}

#[cfg(windows)]
fn open_url(url: &str) -> Result<()> {
    shell_execute("open", url)
}

#[cfg(windows)]
fn run_elevated(path: &std::path::Path) -> Result<()> {
    shell_execute("runas", &path.to_string_lossy())
}

#[cfg(target_os = "macos")]
fn open_url(url: &str) -> Result<()> {
    std::process::Command::new("open")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(Into::into)
}

#[cfg(target_os = "macos")]
fn run_elevated(path: &std::path::Path) -> Result<()> {
    std::process::Command::new("open")
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(Into::into)
}
