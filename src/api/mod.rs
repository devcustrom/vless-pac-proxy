//! HTTP-панель и API (M0-каркас: статика панели + минимальный статус).
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::State;
use axum::response::Html;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::json;

use crate::state::AppState;

const UI_HTML: &str = include_str!("../../ui.html");

pub async fn run(dir: PathBuf) -> anyhow::Result<()> {
    let state = Arc::new(AppState::new(dir.clone()));
    let app = Router::new()
        .route("/", get(index))
        .route("/api/status", get(status))
        .with_state(state);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 8080u16)).await?;
    crate::wlog!("панель: http://127.0.0.1:8080/ (Rust-версия, каркас M0)");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index() -> Html<&'static str> {
    Html(UI_HTML)
}

async fn status(State(st): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(json!({
        "mode": crate::lists::MODE,
        "version": env!("CARGO_PKG_VERSION"),
        "dataDir": st.dir.display().to_string(),
        "settings": st.settings,
        "bootstrap": "M0: API переносится по этапам"
    }))
}
