use sqlx::PgPool;
use uuid::Uuid;

use crate::models::Order;

pub async fn insert_order(
    db: &PgPool,
    id: Uuid,
    customer_id: &str,
    total_cents: i64,
    idempotency_key: Option<&str>,
) -> Result<Order, sqlx::Error> {
    sqlx::query_as::<_, Order>(
        r#"
        INSERT INTO orders (id, customer_id, total_cents, status, idempotency_key)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, customer_id, total_cents, status, idempotency_key, created_at
        "#,
    )
    .bind(id)
    .bind(customer_id)
    .bind(total_cents)
    .bind("PENDING")
    .bind(idempotency_key)
    .fetch_one(db)
    .await
}

pub async fn get_order_by_id(db: &PgPool, id: Uuid) -> Result<Option<Order>, sqlx::Error> {
    sqlx::query_as::<_, Order>(
        r#"
        SELECT id, customer_id, total_cents, status, idempotency_key, created_at
        FROM orders
        WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(db)
    .await
}

pub async fn get_order_by_idempotency_key(
    db: &PgPool,
    key: &str,
) -> Result<Option<Order>, sqlx::Error> {
    sqlx::query_as::<_, Order>(
        r#"
        SELECT id, customer_id, total_cents, status, idempotency_key, created_at
        FROM orders
        WHERE idempotency_key = $1
        "#,
    )
    .bind(key)
    .fetch_optional(db)
    .await
}
