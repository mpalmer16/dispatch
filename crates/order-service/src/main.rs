use order_service::{app::build_app, app_state::AppState};
use sqlx::PgPool;
use std::{env, net::SocketAddr, str::FromStr};
use tracing::info;

#[derive(Debug, PartialEq, Eq)]
struct RuntimeConfig {
    db_url: String,
    bind_addr: SocketAddr,
}

#[tokio::main]
async fn main() {
    init_tracing();

    dotenvy::dotenv().ok();

    let config =
        RuntimeConfig::from_env(|key| env::var(key)).expect("failed to load runtime config");

    let pool = PgPool::connect(&config.db_url)
        .await
        .expect("could not connect to database");

    let state = AppState { db: pool };

    let app = build_app(state);
    info!("starting order-service on {}", config.bind_addr);

    let listener = tokio::net::TcpListener::bind(config.bind_addr)
        .await
        .expect("failed to bind TCP listener");

    axum::serve(listener, app).await.expect("server failed")
}

impl RuntimeConfig {
    fn from_env<GetVar>(get_var: GetVar) -> Result<Self, String>
    where
        GetVar: Fn(&str) -> Result<String, env::VarError>,
    {
        let db_url = get_var("DATABASE_URL").map_err(|_| "DATABASE_URL must be set".to_string())?;
        let bind_addr = get_var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:3000".to_string());
        let bind_addr = SocketAddr::from_str(&bind_addr)
            .map_err(|_| "BIND_ADDR must be a valid socket addr".to_string())?;

        Ok(Self { db_url, bind_addr })
    }
}

fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "order_service=debug,axum=info".to_string()),
        )
        .init()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn runtime_config_uses_default_bind_address_when_unset() {
        let config = RuntimeConfig::from_env(env_map([(
            "DATABASE_URL",
            "postgres://dispatch:dispatch@localhost:5432/dispatch",
        )]))
        .expect("expected config to load");

        assert_eq!(
            config,
            RuntimeConfig {
                db_url: "postgres://dispatch:dispatch@localhost:5432/dispatch".to_string(),
                bind_addr: SocketAddr::from_str("0.0.0.0:3000").unwrap(),
            }
        );
    }

    #[test]
    fn runtime_config_uses_explicit_bind_address() {
        let config = RuntimeConfig::from_env(env_map([
            (
                "DATABASE_URL",
                "postgres://dispatch:dispatch@localhost:5432/dispatch",
            ),
            ("BIND_ADDR", "127.0.0.1:4000"),
        ]))
        .expect("expected config to load");

        assert_eq!(
            config.bind_addr,
            SocketAddr::from_str("127.0.0.1:4000").unwrap()
        );
    }

    #[test]
    fn runtime_config_requires_database_url() {
        let err = RuntimeConfig::from_env(env_map([("BIND_ADDR", "127.0.0.1:4000")]))
            .expect_err("expected missing DATABASE_URL to fail");

        assert_eq!(err, "DATABASE_URL must be set");
    }

    #[test]
    fn runtime_config_rejects_invalid_bind_address() {
        let err = RuntimeConfig::from_env(env_map([
            (
                "DATABASE_URL",
                "postgres://dispatch:dispatch@localhost:5432/dispatch",
            ),
            ("BIND_ADDR", "not-a-socket-addr"),
        ]))
        .expect_err("expected invalid bind address to fail");

        assert_eq!(err, "BIND_ADDR must be a valid socket addr");
    }

    fn env_map<const N: usize>(
        pairs: [(&'static str, &'static str); N],
    ) -> impl Fn(&str) -> Result<String, env::VarError> {
        let values: HashMap<&'static str, &'static str> = pairs.into_iter().collect();

        move |key| {
            values
                .get(key)
                .map(|value| (*value).to_string())
                .ok_or(env::VarError::NotPresent)
        }
    }
}
