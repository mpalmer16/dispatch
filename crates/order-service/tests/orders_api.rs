use axum::{
    Router,
    body::{Body, Bytes},
    http::{Request, Response, StatusCode},
};
use http_body_util::BodyExt;
use order_service::{app::build_app, app_state::AppState, models::Order};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::util::ServiceExt;

#[tokio::test]
async fn create_order_is_idempotent_when_using_the_same_key() {
    let pool = test_pg_pool().await;
    reset_db(&pool).await;

    let state = AppState { db: pool };
    let app = build_app(state);
    let payload = create_payload("customer-test-1", 4999);

    let (status_1, order_1) = do_post_request("/orders", "key-test-1", &payload, &app).await;
    assert_eq!(status_1, StatusCode::CREATED);

    let (status_2, order_2) = do_post_request("/orders", "key-test-1", &payload, &app).await;
    assert_eq!(status_2, StatusCode::OK);

    assert_eq!(order_1.id, order_2.id);
    assert_eq!(order_1.customer_id, order_2.customer_id);
    assert_eq!(order_1.total_cents, order_2.total_cents);
}

#[tokio::test]
async fn same_payload_different_keys_makes_different_orders() {
    let pool = test_pg_pool().await;
    reset_db(&pool).await;

    let state = AppState { db: pool };
    let app = build_app(state);
    let payload = create_payload("customer-test-2", 4999);

    let (status_1, order_1) = do_post_request("/orders", "key-test-2-1", &payload, &app).await;
    assert_eq!(status_1, StatusCode::CREATED);

    let (status_2, order_2) = do_post_request("/orders", "key-test-2-2", &payload, &app).await;
    assert_eq!(status_2, StatusCode::CREATED);

    assert!(order_1.id != order_2.id);
    assert_eq!(order_1.customer_id, order_2.customer_id);
    assert_eq!(order_1.total_cents, order_2.total_cents);
}

#[tokio::test]
async fn create_then_get_by_id_returns_order() {
    let pool = test_pg_pool().await;
    reset_db(&pool).await;

    let state = AppState { db: pool };
    let app = build_app(state);
    let payload = create_payload("customer-test-3", 4999);

    let (status_1, order_1) = do_post_request("/orders", "key-test-3-1", &payload, &app).await;
    assert_eq!(status_1, StatusCode::CREATED);

    let uri = format!("/orders/{}", order_1.id);
    let (status_2, order_2) = do_get_request(&uri, &app).await;

    assert_eq!(status_2, StatusCode::OK);

    assert_eq!(order_1.id, order_2.id);
    assert_eq!(order_1.customer_id, order_2.customer_id);
    assert_eq!(order_1.total_cents, order_2.total_cents);
}

async fn test_pg_pool() -> PgPool {
    dotenvy::dotenv().ok();

    let db_url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL must be set");

    PgPool::connect(&db_url)
        .await
        .expect("failed to connect to test database")
}

async fn reset_db(pool: &PgPool) {
    sqlx::query("TRUNCATE TABLE orders")
        .execute(pool)
        .await
        .expect("failed to trunctate orders table");
}

fn create_payload(customer_id: &str, total_cents: i64) -> Value {
    json!({
        "customer_id": customer_id,
        "total_cents": total_cents
    })
}

fn create_post_request<T: Serialize + ?Sized>(uri: &str, key: &str, payload: &T) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .header("Idempotency-Key", key)
        .body(Body::from(
            serde_json::to_vec(payload).expect("could not serialize payload"),
        ))
        .expect("failed to build request")
}

fn create_get_request(uri: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .body(Body::empty())
        .expect("failed to build request_2")
}

async fn make_request(app: &Router, request: Request<Body>) -> Response<Body> {
    app.clone().oneshot(request).await.expect("request failed")
}

async fn parse_body(response: Response<Body>) -> Bytes {
    response
        .into_body()
        .collect()
        .await
        .expect("failed to read response body")
        .to_bytes()
}

async fn get_status_and_order(app: &Router, request: Request<Body>) -> (StatusCode, Order) {
    let response = make_request(app, request).await;
    let status = response.status();
    let body = parse_body(response).await;
    let order: Order = serde_json::from_slice(&body).expect("failed to deserialize order");
    (status, order)
}

async fn do_post_request<'a, T: Serialize + ?Sized>(
    uri: &'a str,
    key: &'a str,
    payload: &'a T,
    app: &'a Router,
) -> (StatusCode, Order) {
    let request = create_post_request(uri, key, payload);
    get_status_and_order(app, request).await
}

async fn do_get_request(uri: &str, app: &Router) -> (StatusCode, Order) {
    let request = create_get_request(uri);
    get_status_and_order(app, request).await
}
