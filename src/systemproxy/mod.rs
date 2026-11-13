//! Системный прокси: Windows (реестр + WinINET) и macOS (networksetup).
//! Windows-часть готова; macOS — в M1.
pub mod platform;

#[allow(unused_imports)]
pub use platform::{disable_on_exit, get_system_proxy, set_system_proxy};
