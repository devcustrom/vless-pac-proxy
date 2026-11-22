//! Трей Windows/macOS: статус каналов, быстрые переключатели системного
//! прокси и Telegram, открытие панели/настроек и выход с очисткой.
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};
use winit::application::ApplicationHandler;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};

use crate::state::AppState;
use crate::wlog;

pub struct Ctx {
    pub state: Arc<AppState>,
    pub handle: tokio::runtime::Handle,
}

struct MenuHandles {
    status: MenuItem,
    system_proxy: CheckMenuItem,
    telegram: CheckMenuItem,
    telegram_open: MenuItem,
}

struct App {
    ctx: Ctx,
    tray: Option<TrayIcon>,
    menu: Option<MenuHandles>,
    last_refresh: Instant,
}

pub fn run(state: Arc<AppState>, handle: tokio::runtime::Handle) -> Result<()> {
    #[cfg(target_os = "macos")]
    let event_loop = {
        use winit::platform::macos::{ActivationPolicy, EventLoopBuilderExtMacOS};
        let mut builder = EventLoop::builder();
        builder.with_activation_policy(ActivationPolicy::Accessory);
        builder.build()?
    };
    #[cfg(not(target_os = "macos"))]
    let event_loop = EventLoop::new()?;
    let mut app = App {
        ctx: Ctx { state, handle },
        tray: None,
        menu: None,
        last_refresh: Instant::now() - Duration::from_secs(10),
    };
    event_loop.run_app(&mut app)?;
    Ok(())
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.tray.is_none() {
            match build_tray() {
                Ok((tray, menu)) => {
                    self.tray = Some(tray);
                    self.menu = Some(menu);
                    self.refresh_menu();
                    wlog!("трей: меню готово");
                }
                Err(e) => {
                    wlog!("трей: не создан — {e:#}");
                    // На macOS без menu bar приложение становится невидимым.
                    // Показываем причину и выходим, чтобы не оставлять зомби.
                    #[cfg(target_os = "macos")]
                    {
                        crate::macos::show_alert(&format!(
                            "Не удалось создать иконку в menu bar: {e:#}"
                        ));
                        event_loop.exit();
                    }
                    #[cfg(not(target_os = "macos"))]
                    let _ = event_loop;
                }
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
        if self.last_refresh.elapsed() >= Duration::from_secs(3) {
            self.refresh_menu();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(200),
        ));
    }
}

fn build_tray() -> Result<(TrayIcon, MenuHandles)> {
    let status = MenuItem::with_id("status", "VPN: …", false, None);
    let system_proxy =
        CheckMenuItem::with_id("system_proxy", "Системный прокси", true, false, None);
    let telegram = CheckMenuItem::with_id("telegram", "Telegram-прокси", true, false, None);
    let telegram_open = MenuItem::with_id("telegram_open", "Открыть Telegram", false, None);
    let separator1 = PredefinedMenuItem::separator();
    let separator2 = PredefinedMenuItem::separator();
    let separator3 = PredefinedMenuItem::separator();
    let open = MenuItem::with_id("open", "Открыть панель", true, None);
    let settings = MenuItem::with_id("settings", "Настройки…", true, None);
    let exit = MenuItem::with_id("exit", "Выход", true, None);
    let menu = Menu::with_items(&[
        &status,
        &separator1,
        &system_proxy,
        &telegram,
        &telegram_open,
        &separator2,
        &open,
        &settings,
        &separator3,
        &exit,
    ])?;
    let icon = Icon::from_rgba(icon_rgba(), 32, 32)?;
    let builder = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("wireproxy-control — умный прокси")
        .with_icon(icon);
    #[cfg(target_os = "macos")]
    let builder = builder.with_icon_as_template(true);
    let tray = builder.build()?;
    Ok((
        tray,
        MenuHandles {
            status,
            system_proxy,
            telegram,
            telegram_open,
        },
    ))
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
    fn refresh_menu(&mut self) {
        self.last_refresh = Instant::now();
        let settings = self.ctx.state.settings_snapshot();
        let (wg_ok, vless_ok) = self.ctx.state.health_snapshot();
        let status_text = vpn_status_text(
            settings.wireguard_enabled,
            settings.xray_enabled,
            wg_ok,
            vless_ok,
        );
        let system_enabled = crate::systemproxy::get_system_proxy()
            .map(|proxy| proxy.enabled)
            .unwrap_or(false);
        if let Some(menu) = &self.menu {
            menu.status.set_text(&status_text);
            menu.system_proxy.set_checked(system_enabled);
            menu.telegram.set_checked(settings.telegram_enabled);
            menu.telegram_open.set_enabled(settings.telegram_enabled);
        }
        if let Some(tray) = &self.tray {
            let _ = tray.set_tooltip(Some(format!("wireproxy-control · {status_text}")));
        }
    }

    fn toggle_system_proxy(&self) {
        let current = crate::systemproxy::get_system_proxy()
            .map(|proxy| proxy.enabled)
            .unwrap_or(false);
        let enabled = !current;
        if enabled
            && !self
                .ctx
                .state
                .smart_proxy_ready
                .load(std::sync::atomic::Ordering::SeqCst)
        {
            wlog!("трей: умный прокси не слушает 8081 — системный прокси не включаю");
            return;
        }
        let domains: Vec<String> = self
            .ctx
            .state
            .local_domains
            .lock()
            .map(|list| list.iter().map(|item| item.domain.clone()).collect())
            .unwrap_or_default();
        self.ctx.handle.spawn(async move {
            if let Err(e) = crate::systemproxy::set_system_proxy(enabled, &domains) {
                wlog!("трей: системный прокси — {e:#}");
            }
        });
    }

    fn toggle_telegram(&self) {
        let enabled = !self.ctx.state.settings_snapshot().telegram_enabled;
        let state = self.ctx.state.clone();
        // Вызывается из winit-потока, где нет Tokio-контекста:
        // supervisor::start() внутри делает tokio::spawn.
        self.ctx.handle.spawn(async move {
            if let Err(e) = crate::telegram::set_enabled(&state, enabled) {
                wlog!("трей: Telegram-прокси — {e:#}");
            }
        });
    }

    fn open_telegram(&self) {
        let settings = self.ctx.state.settings_snapshot();
        if !settings.telegram_enabled {
            let url = format!("{}#telegram", panel_url(&self.ctx));
            let _ = crate::shell::open_url(&url);
            return;
        }
        match crate::telegram::link::build_link(
            &settings.telegram_host,
            settings.telegram_port,
            &settings.telegram_secret,
            &settings.telegram_fake_tls,
        ) {
            Ok(link) => {
                if let Err(e) = crate::shell::open_url(&link) {
                    wlog!("трей: открыть Telegram — {e:#}");
                }
            }
            Err(e) => {
                wlog!("трей: ссылка Telegram — {e:#}");
                let url = format!("{}#telegram", panel_url(&self.ctx));
                let _ = crate::shell::open_url(&url);
            }
        }
    }

    fn on_menu(&mut self, id: &str) {
        match id {
            "open" => {
                let url = panel_url(&self.ctx);
                if let Err(e) = crate::shell::open_url(&url) {
                    wlog!("трей: открыть панель — {e:#}");
                }
            }
            "settings" => {
                let url = format!("{}#settings", panel_url(&self.ctx));
                if let Err(e) = crate::shell::open_url(&url) {
                    wlog!("трей: настройки — {e:#}");
                }
            }
            "system_proxy" => self.toggle_system_proxy(),
            "telegram" => self.toggle_telegram(),
            "telegram_open" => self.open_telegram(),
            "exit" => {
                wlog!("трей: выход");
                crate::api::cleanup(&self.ctx.state);
                std::thread::sleep(Duration::from_millis(600));
                std::process::exit(0);
            }
            _ => {}
        }
        self.last_refresh = Instant::now() - Duration::from_secs(10);
    }
}

/// Text of the read-only status line in the tray menu.
fn vpn_status_text(wg_enabled: bool, vless_enabled: bool, wg_ok: bool, vless_ok: bool) -> String {
    if !wg_enabled && !vless_enabled {
        return "VPN: выключен".to_string();
    }
    let mut parts = Vec::new();
    if wg_enabled {
        parts.push(if wg_ok {
            "WireGuard ✓"
        } else {
            "WireGuard нет"
        });
    } else {
        parts.push("WireGuard выкл");
    }
    if vless_enabled {
        parts.push(if vless_ok {
            "VLESS ✓"
        } else {
            "VLESS нет"
        });
    } else {
        parts.push("VLESS выкл");
    }
    format!("VPN: {}", parts.join(" · "))
}

/// Выбор URL панели: локальный домен только если плагин включён, CA доверен,
/// HTTPS-слушатель без ошибки и есть mapping на порт панели.
fn choose_panel_url(
    enabled: bool,
    domains: &[crate::net::local_proxy::LocalDomain],
    panel_port: u16,
    https_port: u16,
    trusted: bool,
    https_error: bool,
) -> String {
    let loopback = || format!("http://127.0.0.1:{panel_port}/");
    if !enabled || !trusted || https_error {
        return loopback();
    }
    let selected = domains
        .iter()
        .find(|item| item.port == panel_port && item.domain == "panel.localhost")
        .or_else(|| domains.iter().find(|item| item.port == panel_port));
    let Some(item) = selected else {
        return loopback();
    };
    if https_port == 443 {
        format!("https://{}/", item.domain)
    } else {
        format!("https://{}:{https_port}/", item.domain)
    }
}

fn panel_url(ctx: &Ctx) -> String {
    let settings = ctx.state.settings_snapshot();
    let domains = ctx
        .state
        .local_domains
        .lock()
        .map(|items| items.clone())
        .unwrap_or_default();
    let https_error = ctx
        .state
        .local_https_error
        .lock()
        .map(|value| value.is_some())
        .unwrap_or(true);
    choose_panel_url(
        settings.local_domains_enabled,
        &domains,
        ctx.state.panel_port,
        settings.local_https_port,
        crate::trust_status::is_trusted(&ctx.state.dir),
        https_error,
    )
}

#[cfg(test)]
mod tests {
    use super::{choose_panel_url, vpn_status_text};
    use crate::net::local_proxy::LocalDomain;

    fn domains() -> Vec<LocalDomain> {
        vec![
            LocalDomain {
                domain: "panel.localhost".into(),
                port: 8080,
            },
            LocalDomain {
                domain: "app.example.com".into(),
                port: 8080,
            },
        ]
    }

    #[test]
    fn plugin_off_uses_loopback() {
        assert_eq!(
            choose_panel_url(false, &domains(), 8080, 443, true, false),
            "http://127.0.0.1:8080/"
        );
    }

    #[test]
    fn trusted_panel_domain() {
        assert_eq!(
            choose_panel_url(true, &domains(), 8080, 443, true, false),
            "https://panel.localhost/"
        );
    }

    #[test]
    fn custom_domain_without_panel_localhost() {
        let list = vec![LocalDomain {
            domain: "panel.example.com".into(),
            port: 8080,
        }];
        assert_eq!(
            choose_panel_url(true, &list, 8080, 8443, true, false),
            "https://panel.example.com:8443/"
        );
    }

    #[test]
    fn https_error_falls_back() {
        assert_eq!(
            choose_panel_url(true, &domains(), 8080, 443, true, true),
            "http://127.0.0.1:8080/"
        );
    }

    #[test]
    fn tray_status_text() {
        assert_eq!(
            vpn_status_text(true, true, true, false),
            "VPN: WireGuard ✓ · VLESS нет"
        );
        assert_eq!(
            vpn_status_text(false, true, true, true),
            "VPN: WireGuard выкл · VLESS ✓"
        );
        assert_eq!(vpn_status_text(false, false, false, false), "VPN: выключен");
    }
}
