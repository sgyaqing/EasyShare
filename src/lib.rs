pub mod avatar;
pub mod db;
pub mod handlers;
pub mod i18n;
pub mod identity;
pub mod models;
pub mod sse;
pub mod state;

use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use axum::Router;
use tower_http::services::ServeDir;

use state::AppState;

/// Upload size cap. Axum's default is 2 MiB — far too small for a
/// file-sharing tool.
pub const MAX_BODY_SIZE: usize = 1024 * 1024 * 1024; // 1 GiB

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(handlers::index))
        .route("/api/me", get(handlers::me))
        .route("/api/messages", get(handlers::get_messages).post(handlers::post_message))
        .route("/api/messages/{id}/recall", post(handlers::recall_message))
        .route("/api/upload", post(handlers::upload))
        .route("/api/events", get(sse::events))
        .route("/icon.svg", get(handlers::icon_svg))
        .route("/icon-32.png", get(handlers::icon_png_32))
        .nest_service("/files", ServeDir::new(state.data_dir.clone()))
        .layer(DefaultBodyLimit::max(MAX_BODY_SIZE))
        .with_state(state)
}
