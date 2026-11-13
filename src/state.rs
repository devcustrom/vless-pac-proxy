//! Общее состояние приложения (будет расширяться по мере переноса функций).
use std::path::PathBuf;

use crate::settings::Settings;

pub struct AppState {
    pub dir: PathBuf,
    pub settings: Settings,
}

impl AppState {
    pub fn new(dir: PathBuf) -> Self {
        let settings = Settings::load(&dir);
        Self { dir, settings }
    }
}
