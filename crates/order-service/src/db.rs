use sqlx::PgPool;
use uuid::Uuid;

use crate::models::Order;

pub async fn insert_order(
    db: &PgPool,
    id: Uuid,
    customer_id: &str,
    total_cents: i64,
) -> Result<Order, sqlx::Error> {
    sqlx::query_as::<_, Order>(
        r#"
        INSERT INTO orders (id, customer_id, total_cents, status)
        VALUES ($1, $2, $3, $4)
        RETURNING id, customer_id, total_cents, status, created_at
        "#,
    )
    .bind(id)
    .bind(customer_id)
    .bind(total_cents)
    .bind("PENDING")
    .fetch_one(db)
    .await
}

pub async fn get_order_by_id(db: &PgPool, id: Uuid) -> Result<Option<Order>, sqlx::Error> {
    sqlx::query_as::<_, Order>(
        r#"
        SELECT id, customer_id, total_cents, status, created_at
        FROM orders
        WHERE id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(db)
    .await
}
