use sqlx::postgres::PgPoolOptions;
use std::{env, time::Duration};
use tokio::time::sleep;
use tracing::{info, warn};

const MAX_ATTEMPTS: u32 = 30;
const RETRY_DELAY: Duration = Duration::from_secs(2);

#[tokio::main]
async fn main() {
    init_tracing();

    dotenvy::dotenv().ok();

    let db_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let pool = connect_with_retry(&db_url).await;

    info!("running sqlx migrations");
    sqlx::migrate!("../../migrations")
        .run(&pool)
        .await
        .expect("failed to run migrations");

    info!("migrations completed successfully");
}

async fn connect_with_retry(db_url: &str) -> sqlx::PgPool {
    for attempt in 1..=MAX_ATTEMPTS {
        match PgPoolOptions::new().max_connections(1).connect(db_url).await {
            Ok(pool) => {
                info!("connected to database on attempt {}", attempt);
                return pool;
            }
            Err(error) if attempt < MAX_ATTEMPTS => {
                warn!(
                    "database not ready yet on attempt {}: {}. retrying in {:?}",
                    attempt, error, RETRY_DELAY
                );
                sleep(RETRY_DELAY).await;
            }
            Err(error) => {
                panic!(
                    "failed to connect to database after {} attempts: {}",
                    MAX_ATTEMPTS, error
                );
            }
        }
    }

    unreachable!("database retry loop exited unexpectedly");
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "migrate=info,sqlx=warn".to_string()),
        )
        .init();
}
