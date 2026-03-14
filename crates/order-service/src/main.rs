use axum::{
    Router,
    routing::{get, post},
};
use sqlx::PgPool;
use std::{env, net::SocketAddr};
use tracing::info;

mod app_state;
mod db;
mod handlers;
mod models;
use app_state::AppState;

use crate::handlers::orders::{create_order, get_order};

#[tokio::main]
async fn main() {
    init_tracing();

    dotenvy::dotenv().ok();

    let db_url = env::var("DATABASE_URL").expect("database url must be set");

    let pool = PgPool::connect(&db_url)
        .await
        .expect("could not connect to database");

    let state = AppState { db: pool };

    let app = build_app(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    info!("starting order-service on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind TCP listener");

    axum::serve(listener, app).await.expect("server failed")
}

fn build_app(app_state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/orders", post(create_order))
        .route("/orders/{id}", get(get_order))
        .with_state(app_state)
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "order_service=debug,axum=info".to_string()),
        )
        .init()
}

async fn health() -> &'static str {
    "OK"
}
