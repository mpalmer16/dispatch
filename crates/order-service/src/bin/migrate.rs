use sqlx::postgres::PgPoolOptions;
use std::{env, future::Future, time::Duration};
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
    retry_with_delay(
        MAX_ATTEMPTS,
        RETRY_DELAY,
        |attempt| async move {
            match PgPoolOptions::new()
                .max_connections(1)
                .connect(db_url)
                .await
            {
                Ok(pool) => {
                    info!("connected to database on attempt {}", attempt);
                    Ok(pool)
                }
                Err(error) => {
                    if attempt < MAX_ATTEMPTS {
                        warn!(
                            "database not ready yet on attempt {}: {}. retrying in {:?}",
                            attempt, error, RETRY_DELAY
                        );
                    }
                    Err(error)
                }
            }
        },
        |delay| async move { sleep(delay).await },
    )
    .await
    .map(|(pool, _attempt)| pool)
    .unwrap_or_else(|error| {
        panic!(
            "failed to connect to database after {} attempts: {}",
            MAX_ATTEMPTS, error
        )
    })
}

async fn retry_with_delay<T, E, Operation, OperationFuture, Pause, PauseFuture>(
    max_attempts: u32,
    retry_delay: Duration,
    mut operation: Operation,
    mut pause: Pause,
) -> Result<(T, u32), E>
where
    Operation: FnMut(u32) -> OperationFuture,
    OperationFuture: Future<Output = Result<T, E>>,
    Pause: FnMut(Duration) -> PauseFuture,
    PauseFuture: Future<Output = ()>,
{
    for attempt in 1..=max_attempts {
        match operation(attempt).await {
            Ok(value) => return Ok((value, attempt)),
            Err(_error) if attempt < max_attempts => pause(retry_delay).await,
            Err(error) => return Err(error),
        }
    }

    unreachable!("retry loop exited unexpectedly");
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "migrate=info,sqlx=warn".to_string()),
        )
        .init();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
    };

    #[tokio::test]
    async fn retry_with_delay_returns_immediate_success_without_pausing() {
        let pauses = Arc::new(Mutex::new(Vec::new()));

        let result = retry_with_delay(
            3,
            Duration::from_millis(5),
            |_attempt| async { Ok::<_, &'static str>("connected") },
            {
                let pauses = Arc::clone(&pauses);
                move |delay| {
                    let pauses = Arc::clone(&pauses);
                    async move { pauses.lock().unwrap().push(delay) }
                }
            },
        )
        .await;

        assert_eq!(result, Ok(("connected", 1)));
        assert!(pauses.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn retry_with_delay_retries_until_success() {
        let attempts = Arc::new(AtomicU32::new(0));
        let pauses = Arc::new(Mutex::new(Vec::new()));

        let result = retry_with_delay(
            5,
            Duration::from_millis(10),
            {
                let attempts = Arc::clone(&attempts);
                move |_attempt| {
                    let attempts = Arc::clone(&attempts);
                    async move {
                        let current = attempts.fetch_add(1, Ordering::SeqCst) + 1;
                        if current < 3 {
                            Err("not ready")
                        } else {
                            Ok("connected")
                        }
                    }
                }
            },
            {
                let pauses = Arc::clone(&pauses);
                move |delay| {
                    let pauses = Arc::clone(&pauses);
                    async move { pauses.lock().unwrap().push(delay) }
                }
            },
        )
        .await;

        assert_eq!(result, Ok(("connected", 3)));
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
        assert_eq!(
            pauses.lock().unwrap().as_slice(),
            &[Duration::from_millis(10); 2]
        );
    }

    #[tokio::test]
    async fn retry_with_delay_returns_last_error_after_max_attempts() {
        let attempts = Arc::new(AtomicU32::new(0));
        let pauses = Arc::new(Mutex::new(Vec::new()));

        let result = retry_with_delay(
            3,
            Duration::from_millis(20),
            {
                let attempts = Arc::clone(&attempts);
                move |_attempt| {
                    let attempts = Arc::clone(&attempts);
                    async move {
                        attempts.fetch_add(1, Ordering::SeqCst);
                        Err::<(), _>("still not ready")
                    }
                }
            },
            {
                let pauses = Arc::clone(&pauses);
                move |delay| {
                    let pauses = Arc::clone(&pauses);
                    async move { pauses.lock().unwrap().push(delay) }
                }
            },
        )
        .await;

        assert_eq!(result, Err("still not ready"));
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
        assert_eq!(
            pauses.lock().unwrap().as_slice(),
            &[Duration::from_millis(20); 2]
        );
    }
}
