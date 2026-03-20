use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::models::{Order, OrderCreatedEvent, OrderOutbox};

pub async fn insert_order(
    tx: &mut Transaction<'_, Postgres>,
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
    .fetch_one(tx.as_mut())
    .await
}

pub async fn insert_outbox(
    tx: &mut Transaction<'_, Postgres>,
    aggregate_id: Uuid,
    event: &OrderCreatedEvent,
) -> Result<OrderOutbox, sqlx::Error> {
    let payload = serde_json::to_value(event).expect("failed to serialize order created event");

    sqlx::query_as::<_, OrderOutbox>(
        r#"
        INSERT INTO order_outbox (id, aggregate_id, event_type, payload, processed_at)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING id, aggregate_id, event_type, payload, created_at, processed_at
        "#,
    )
    .bind(event.event_id)
    .bind(aggregate_id)
    .bind(&event.event_type)
    .bind(payload)
    .bind(Option::<chrono::DateTime<chrono::Utc>>::None)
    .fetch_one(tx.as_mut())
    .await
}

pub async fn insert_order_with_outbox(
    db: &PgPool,
    order_id: Uuid,
    customer_id: &str,
    total_cents: i64,
    idempotency_key: Option<&str>,
    event: &OrderCreatedEvent,
) -> Result<Order, sqlx::Error> {
    let mut tx = db.begin().await?;

    let order = insert_order(&mut tx, order_id, customer_id, total_cents, idempotency_key).await?;
    let _ = insert_outbox(&mut tx, order.id, event).await?;

    tx.commit().await?;
    Ok(order)
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

pub async fn get_order_by_idempotency_key_and_customer_id(
    db: &PgPool,
    key: &str,
    customer_id: &str,
) -> Result<Option<Order>, sqlx::Error> {
    sqlx::query_as::<_, Order>(
        r#"
        SELECT id, customer_id, total_cents, status, idempotency_key, created_at
        FROM orders
        WHERE idempotency_key = $1
        AND customer_id = $2
        "#,
    )
    .bind(key)
    .bind(customer_id)
    .fetch_optional(db)
    .await
}
