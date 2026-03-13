use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use uuid::Uuid;

use crate::{
    app_state::AppState,
    db,
    models::{CreateOrderRequest, Order},
};

pub async fn create_order(
    State(state): State<AppState>,
    Json(payload): Json<CreateOrderRequest>,
) -> Result<(StatusCode, Json<Order>), StatusCode> {
    let order = db::insert_order(
        &state.db,
        Uuid::new_v4(),
        &payload.customer_id,
        payload.total_cents,
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

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
