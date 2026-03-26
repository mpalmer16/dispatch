use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use chrono::Utc;
use std::future::Future;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    app_state::AppState,
    db,
    models::{CreateOrderRequest, Order, OrderCreatedEvent, OrderCreatedPayload},
};

pub async fn create_order(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateOrderRequest>,
) -> Result<(StatusCode, Json<Order>), StatusCode> {
    let lookup_db = state.db.clone();
    let insert_db = state.db.clone();

    create_order_with(
        headers,
        payload,
        |key, customer_id| {
            let db = lookup_db.clone();
            async move {
                db::get_order_by_idempotency_key_and_customer_id(&db, &key, &customer_id).await
            }
        },
        |order_id, customer_id, total_cents, idempotency_key, event| {
            let db = insert_db.clone();
            async move {
                db::insert_order_with_outbox(
                    &db,
                    order_id,
                    &customer_id,
                    total_cents,
                    idempotency_key.as_deref(),
                    &event,
                )
                .await
            }
        },
    )
    .await
}

async fn create_order_with<Lookup, LookupFuture, Insert, InsertFuture>(
    headers: HeaderMap,
    payload: CreateOrderRequest,
    lookup_order: Lookup,
    insert_order: Insert,
) -> Result<(StatusCode, Json<Order>), StatusCode>
where
    Lookup: Fn(String, String) -> LookupFuture,
    LookupFuture: Future<Output = Result<Option<Order>, sqlx::Error>>,
    Insert: Fn(Uuid, String, i64, Option<String>, OrderCreatedEvent) -> InsertFuture,
    InsertFuture: Future<Output = Result<Order, sqlx::Error>>,
{
    let idempotency_key = headers
        .get("Idempotency-Key")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);

    if let Some(key) = idempotency_key.as_ref() {
        if let Some(existing) = lookup_order(key.clone(), payload.customer_id.clone())
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        {
            return if payload.total_cents == existing.total_cents {
                info!(
                    order_id = %existing.id,
                    customer_id = %existing.customer_id,
                    total_cents = existing.total_cents,
                    idempotency_key = key,
                    "reused existing order for idempotent create request"
                );
                Ok((StatusCode::OK, Json(existing)))
            } else {
                warn!(
                    customer_id = %payload.customer_id,
                    requested_total_cents = payload.total_cents,
                    existing_total_cents = existing.total_cents,
                    idempotency_key = key,
                    "rejected conflicting create request for existing idempotency key"
                );
                Err(StatusCode::CONFLICT)
            };
        }
    }

    let order_id = Uuid::new_v4();
    let event_id = Uuid::new_v4();
    let occured_at = Utc::now();

    let event = OrderCreatedEvent {
        event_id,
        event_type: "order.created".to_string(),
        occurred_at: occured_at,
        payload: OrderCreatedPayload {
            order_id,
            customer_id: payload.customer_id.clone(),
            total_cents: payload.total_cents,
            status: "created".to_string(),
        },
    };

    let order = insert_order(
        order_id,
        payload.customer_id,
        payload.total_cents,
        idempotency_key,
        event.clone(),
    )
    .await
    .map_err(|_| StatusCode::GATEWAY_TIMEOUT)?;

    info!(
        order_id = %order.id,
        customer_id = %order.customer_id,
        total_cents = order.total_cents,
        event_id = %event_id,
        "created order and wrote outbox event"
    );

    Ok((StatusCode::CREATED, Json(order)))
}

pub async fn get_order(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Order>, StatusCode> {
    let db = state.db.clone();
    get_order_with(id, |id| {
        let db = db.clone();
        async move { db::get_order_by_id(&db, id).await }
    })
    .await
}

async fn get_order_with<Get, GetFuture>(id: Uuid, get_order: Get) -> Result<Json<Order>, StatusCode>
where
    Get: Fn(Uuid) -> GetFuture,
    GetFuture: Future<Output = Result<Option<Order>, sqlx::Error>>,
{
    let order = get_order(id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match order {
        Some(order) => Ok(Json(order)),
        None => Err(StatusCode::NOT_FOUND),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[tokio::test]
    async fn create_order_maps_lookup_failures_to_internal_server_error() {
        let result = create_order_with(
            idempotency_headers(Some("key-test-lookup-error")),
            create_order_request("customer-test-lookup-error", 4999),
            |_key, _customer_id| async { Err(sqlx::Error::Protocol("lookup failed".into())) },
            |_order_id, _customer_id, _total_cents, _idempotency_key, _event| async {
                panic!("insert should not be called when lookup fails")
            },
        )
        .await;

        assert!(matches!(result, Err(StatusCode::INTERNAL_SERVER_ERROR)));
    }

    #[tokio::test]
    async fn create_order_maps_insert_failures_to_gateway_timeout() {
        let result = create_order_with(
            idempotency_headers(Some("key-test-insert-error")),
            create_order_request("customer-test-insert-error", 4999),
            |_key, _customer_id| async { Ok(None) },
            |_order_id, _customer_id, _total_cents, _idempotency_key, _event| async {
                Err(sqlx::Error::Protocol("insert failed".into()))
            },
        )
        .await;

        assert!(matches!(result, Err(StatusCode::GATEWAY_TIMEOUT)));
    }

    #[tokio::test]
    async fn get_order_maps_lookup_failures_to_internal_server_error() {
        let result = get_order_with(Uuid::new_v4(), |_id| async {
            Err(sqlx::Error::Protocol("get failed".into()))
        })
        .await;

        assert!(matches!(result, Err(StatusCode::INTERNAL_SERVER_ERROR)));
    }

    #[tokio::test]
    async fn get_order_returns_not_found_when_store_has_no_match() {
        let result = get_order_with(Uuid::new_v4(), |_id| async { Ok(None) }).await;

        assert!(matches!(result, Err(StatusCode::NOT_FOUND)));
    }

    #[tokio::test]
    async fn create_order_returns_existing_order_for_matching_idempotent_request() {
        let existing = sample_order("customer-test-existing", 4999, Some("key-test-existing"));

        let result = create_order_with(
            idempotency_headers(Some("key-test-existing")),
            create_order_request("customer-test-existing", 4999),
            move |_key, _customer_id| {
                let existing = sample_order_from(&existing);
                async move { Ok(Some(existing)) }
            },
            |_order_id, _customer_id, _total_cents, _idempotency_key, _event| async {
                panic!("insert should not be called for matching idempotent requests")
            },
        )
        .await;

        let (status, Json(order)) =
            result.expect("expected matching idempotent request to succeed");
        assert_eq!(status, StatusCode::OK);
        assert_eq!(order.customer_id, "customer-test-existing");
        assert_eq!(order.total_cents, 4999);
    }

    fn idempotency_headers(key: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(key) = key {
            headers.insert(
                "Idempotency-Key",
                key.parse().expect("invalid header value"),
            );
        }
        headers
    }

    fn create_order_request(customer_id: &str, total_cents: i64) -> CreateOrderRequest {
        CreateOrderRequest {
            customer_id: customer_id.to_string(),
            total_cents,
        }
    }

    fn sample_order(customer_id: &str, total_cents: i64, idempotency_key: Option<&str>) -> Order {
        Order {
            id: Uuid::new_v4(),
            customer_id: customer_id.to_string(),
            total_cents,
            status: "PENDING".to_string(),
            idempotency_key: idempotency_key.map(str::to_owned),
            created_at: Utc::now(),
        }
    }

    fn sample_order_from(order: &Order) -> Order {
        Order {
            id: order.id,
            customer_id: order.customer_id.clone(),
            total_cents: order.total_cents,
            status: order.status.clone(),
            idempotency_key: order.idempotency_key.clone(),
            created_at: order.created_at,
        }
    }
}
