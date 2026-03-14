use axum::{
    Router,
    routing::{get, post},
};

use crate::{
    app_state::AppState,
    handlers::orders::{create_order, get_order},
};

pub fn build_app(app_state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/orders", post(create_order))
        .route("/orders/{id}", get(get_order))
        .with_state(app_state)
}

async fn health() -> &'static str {
    "OK"
}
