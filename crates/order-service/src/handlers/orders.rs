use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use chrono::Utc;
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
    let idempotency_key = headers.get("Idempotency-Key").and_then(|v| v.to_str().ok());

    if let Some(key) = idempotency_key {
        if let Some(existing) =
            db::get_order_by_idempotency_key_and_customer_id(&state.db, key, &payload.customer_id)
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

    let order = db::insert_order_with_outbox(
        &state.db,
        order_id,
        &payload.customer_id,
        payload.total_cents,
        idempotency_key,
        &event,
    )
    .await
    .map_err(|_| StatusCode::GATEWAY_TIMEOUT)?;

    info!(
        order_id = %order.id,
        customer_id = %order.customer_id,
        total_cents = order.total_cents,
        event_id = %event.event_id,
        "created order and wrote outbox event"
    );

    Ok((StatusCode::CREATED, Json(order)))
}

pub async fn get_order(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Order>, StatusCode> {
    let order = db::get_order_by_id(&state.db, id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    match order {
        Some(order) => Ok(Json(order)),
        None => Err(StatusCode::NOT_FOUND),
    }
}
