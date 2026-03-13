use axum::{Router, routing::get};
use std::net::SocketAddr;
use tracing::info;

#[tokio::main]
async fn main() {
    init_tracing();

    let app = Router::new().route("/health", get(health));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    info!("starting order-service on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind TCP listener");

    axum::serve(listener, app).await.expect("server failed")
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
