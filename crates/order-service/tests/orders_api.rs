use axum::{
    Router,
    body::{Body, Bytes},
    http::{Request, Response, StatusCode},
};
use http_body_util::BodyExt;
use order_service::{
    app::build_app,
    app_state::AppState,
    models::{Order, OrderCreatedEvent, OrderOutbox},
};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::OnceLock;
use tower::util::ServiceExt;
use uuid::Uuid;

static TEST_MUTEX: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

#[tokio::test]
async fn create_order_is_idempotent_when_using_the_same_key() {
    let _guard = test_lock().await;
    let (app, _pool) = test_setup().await;
    let payload = create_payload("customer-test-1", 4999);

    let (status_1, order_1) = do_post_request("/orders", Some("key-test-1"), &payload, &app).await;
    assert_eq!(status_1, StatusCode::CREATED);
    let order_1 = order_1.expect("expected first POST to return an order");

    let (status_2, order_2) = do_post_request("/orders", Some("key-test-1"), &payload, &app).await;
    assert_eq!(status_2, StatusCode::OK);
    let order_2 = order_2.expect("expected second POST to return an order");

    assert_eq!(order_1.id, order_2.id);
    assert_eq!(order_1.customer_id, order_2.customer_id);
    assert_eq!(order_1.total_cents, order_2.total_cents);
}

#[tokio::test]
async fn same_payload_different_keys_makes_different_orders() {
    let _guard = test_lock().await;
    let (app, _pool) = test_setup().await;
    let payload = create_payload("customer-test-2", 4999);

    let (status_1, order_1) =
        do_post_request("/orders", Some("key-test-2-1"), &payload, &app).await;
    assert_eq!(status_1, StatusCode::CREATED);
    let order_1 = order_1.expect("expected first POST to return an order");

    let (status_2, order_2) =
        do_post_request("/orders", Some("key-test-2-2"), &payload, &app).await;
    assert_eq!(status_2, StatusCode::CREATED);
    let order_2 = order_2.expect("expected second POST to return an order");

    assert_ne!(order_1.id, order_2.id);
    assert_eq!(order_1.customer_id, order_2.customer_id);
    assert_eq!(order_1.total_cents, order_2.total_cents);
}

#[tokio::test]
async fn different_payload_different_keys_makes_different_orders() {
    let _guard = test_lock().await;
    let (app, _pool) = test_setup().await;
    let payload_1 = create_payload("customer-test-3-1", 4999);
    let payload_2 = create_payload("customer-test-3-2", 5999);

    let (status_1, order_1) =
        do_post_request("/orders", Some("key-test-3-1"), &payload_1, &app).await;
    assert_eq!(status_1, StatusCode::CREATED);
    let order_1 = order_1.expect("expected first POST to return an order");

    let (status_2, order_2) =
        do_post_request("/orders", Some("key-test-3-2"), &payload_2, &app).await;
    assert_eq!(status_2, StatusCode::CREATED);
    let order_2 = order_2.expect("expected second POST to return an order");

    assert_ne!(order_1.id, order_2.id);
}

#[tokio::test]
async fn create_then_get_by_id_returns_order() {
    let _guard = test_lock().await;
    let (app, _pool) = test_setup().await;
    let payload = create_payload("customer-test-4", 4999);

    let (status_1, order_1) =
        do_post_request("/orders", Some("key-test-4-1"), &payload, &app).await;
    assert_eq!(status_1, StatusCode::CREATED);
    let order_1 = order_1.expect("expected POST to return an order");

    let uri = format!("/orders/{}", order_1.id);
    let (status_2, order_2) = do_get_request(&uri, &app).await;
    let order_2 = order_2.expect("expected GET to return an order");

    assert_eq!(status_2, StatusCode::OK);

    assert_eq!(order_1.id, order_2.id);
    assert_eq!(order_1.customer_id, order_2.customer_id);
    assert_eq!(order_1.total_cents, order_2.total_cents);
}

#[tokio::test]
async fn same_customer_same_key_different_payloads_throws_conflict() {
    let _guard = test_lock().await;
    let (app, _pool) = test_setup().await;
    let payload_1 = create_payload("customer-test-5", 4999);
    let payload_2 = create_payload("customer-test-5", 5999);

    let (status_1, _) = do_post_request("/orders", Some("key-test-5"), &payload_1, &app).await;
    assert_eq!(status_1, StatusCode::CREATED);

    let (status_2, order_2) =
        do_post_request("/orders", Some("key-test-5"), &payload_2, &app).await;
    assert_eq!(status_2, StatusCode::CONFLICT);
    assert!(order_2.is_none())
}

#[tokio::test]
async fn different_customer_same_key_same_payloads_creates_order() {
    let _guard = test_lock().await;
    let (app, _pool) = test_setup().await;
    let payload_1 = create_payload("customer-test-6", 4999);
    let payload_2 = create_payload("customer-test-7", 4999);

    let (status_1, _) = do_post_request("/orders", Some("key-test-6"), &payload_1, &app).await;
    assert_eq!(status_1, StatusCode::CREATED);

    let (status_2, _) = do_post_request("/orders", Some("key-test-6"), &payload_2, &app).await;
    assert_eq!(status_2, StatusCode::CREATED);
}

#[tokio::test]
async fn no_key_repeated_makes_different_ids() {
    let _guard = test_lock().await;
    let (app, _pool) = test_setup().await;
    let payload_1 = create_payload("customer-test-8", 4999);

    let (status_1, order_1) = do_post_request("/orders", None, &payload_1, &app).await;
    assert_eq!(status_1, StatusCode::CREATED);
    let order_1 = order_1.expect("expected first POST to return an order");

    let (status_2, order_2) = do_post_request("/orders", None, &payload_1, &app).await;
    assert_eq!(status_2, StatusCode::CREATED);
    let order_2 = order_2.expect("expected second POST to return an order");

    assert_ne!(order_1.id, order_2.id);
}

#[tokio::test]
async fn create_order_writes_matching_outbox_event() {
    let _guard = test_lock().await;
    let (app, pool) = test_setup().await;
    let payload = create_payload("customer-test-9", 4999);

    let (status, order) = do_post_request("/orders", Some("key-test-9"), &payload, &app).await;
    assert_eq!(status, StatusCode::CREATED);
    let order = order.expect("expected POST to return an order");

    assert_eq!(order_count(&pool).await, 1);
    assert_eq!(outbox_count(&pool).await, 1);

    let outbox = fetch_outbox_event_for_order(&pool, order.id).await;
    assert_eq!(
        outbox.id,
        serde_json::from_value::<OrderCreatedEvent>(outbox.payload.clone())
            .unwrap()
            .event_id
    );
    assert_eq!(outbox.aggregate_id, order.id);
    assert_eq!(outbox.event_type, "order.created");
    assert!(outbox.processed_at.is_none());

    let event: OrderCreatedEvent =
        serde_json::from_value(outbox.payload).expect("expected valid order created event payload");
    assert_eq!(event.event_type, "order.created");
    assert_eq!(event.payload.order_id, order.id);
    assert_eq!(event.payload.customer_id, order.customer_id);
    assert_eq!(event.payload.total_cents, order.total_cents);
    assert_eq!(event.payload.status, "created");
}

#[tokio::test]
async fn idempotent_retry_does_not_write_second_outbox_event() {
    let _guard = test_lock().await;
    let (app, pool) = test_setup().await;
    let payload = create_payload("customer-test-10", 4999);

    let (status_1, order_1) = do_post_request("/orders", Some("key-test-10"), &payload, &app).await;
    assert_eq!(status_1, StatusCode::CREATED);
    let order_1 = order_1.expect("expected first POST to return an order");
    assert_eq!(outbox_count(&pool).await, 1);

    let (status_2, order_2) = do_post_request("/orders", Some("key-test-10"), &payload, &app).await;
    assert_eq!(status_2, StatusCode::OK);
    let order_2 = order_2.expect("expected second POST to return an order");

    assert_eq!(order_1.id, order_2.id);
    assert_eq!(outbox_count(&pool).await, 1);
}

#[tokio::test]
async fn conflicting_retry_does_not_write_second_outbox_event() {
    let _guard = test_lock().await;
    let (app, pool) = test_setup().await;
    let payload_1 = create_payload("customer-test-11", 4999);
    let payload_2 = create_payload("customer-test-11", 5999);

    let (status_1, _) = do_post_request("/orders", Some("key-test-11"), &payload_1, &app).await;
    assert_eq!(status_1, StatusCode::CREATED);
    assert_eq!(outbox_count(&pool).await, 1);

    let (status_2, order_2) =
        do_post_request("/orders", Some("key-test-11"), &payload_2, &app).await;
    assert_eq!(status_2, StatusCode::CONFLICT);
    assert!(order_2.is_none());
    assert_eq!(outbox_count(&pool).await, 1);
}

#[tokio::test]
async fn get_order_returns_not_found_for_unknown_id() {
    let _guard = test_lock().await;
    let (app, _pool) = test_setup().await;

    let uri = format!("/orders/{}", Uuid::new_v4());
    let (status, order) = do_get_request(&uri, &app).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(order.is_none());
}

#[tokio::test]
async fn health_returns_ok() {
    let _guard = test_lock().await;
    let (app, _pool) = test_setup().await;

    let (status, body) = do_get_body_request("/health", &app).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_ref(), b"OK");
}

#[tokio::test]
async fn negative_total_returns_gateway_timeout_and_writes_nothing() {
    let _guard = test_lock().await;
    let (app, pool) = test_setup().await;
    let payload = create_payload("customer-test-12", -1);

    let (status, order) = do_post_request("/orders", Some("key-test-12"), &payload, &app).await;

    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
    assert!(order.is_none());
    assert_eq!(order_count(&pool).await, 0);
    assert_eq!(outbox_count(&pool).await, 0);
}

#[tokio::test]
async fn malformed_json_returns_unprocessable_entity() {
    let _guard = test_lock().await;
    let (app, pool) = test_setup().await;

    let (status, _body) = do_post_raw_request(
        "/orders",
        Some("key-test-13"),
        r#"{"customer_id":"customer-test-13","total_cents":"oops""#,
        &app,
    )
    .await;

    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(order_count(&pool).await, 0);
    assert_eq!(outbox_count(&pool).await, 0);
}

#[tokio::test]
async fn get_order_with_invalid_uuid_returns_bad_request() {
    let _guard = test_lock().await;
    let (app, _pool) = test_setup().await;

    let (status, _body) = do_get_body_request("/orders/not-a-uuid", &app).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

async fn test_setup() -> (Router, PgPool) {
    let pool = test_pg_pool().await;
    reset_db(&pool).await;

    let state = AppState { db: pool.clone() };
    (build_app(state), pool)
}

async fn test_pg_pool() -> PgPool {
    dotenvy::dotenv().ok();

    let db_url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL must be set");

    PgPool::connect(&db_url)
        .await
        .expect("failed to connect to test database")
}

async fn reset_db(pool: &PgPool) {
    sqlx::query("TRUNCATE TABLE order_outbox, orders")
        .execute(pool)
        .await
        .expect("failed to truncate test tables");
}

async fn order_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM orders")
        .fetch_one(pool)
        .await
        .expect("failed to count orders")
}

async fn outbox_count(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM order_outbox")
        .fetch_one(pool)
        .await
        .expect("failed to count outbox rows")
}

async fn fetch_outbox_event_for_order(pool: &PgPool, order_id: Uuid) -> OrderOutbox {
    sqlx::query_as::<_, OrderOutbox>(
        r#"
        SELECT id, aggregate_id, event_type, payload, created_at, processed_at
        FROM order_outbox
        WHERE aggregate_id = $1
        "#,
    )
    .bind(order_id)
    .fetch_one(pool)
    .await
    .expect("failed to fetch outbox row for order")
}

async fn test_lock() -> tokio::sync::MutexGuard<'static, ()> {
    TEST_MUTEX
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await
}

fn create_payload(customer_id: &str, total_cents: i64) -> Value {
    json!({
        "customer_id": customer_id,
        "total_cents": total_cents
    })
}

fn create_post_request<T: Serialize + ?Sized>(
    uri: &str,
    key: Option<&str>,
    payload: &T,
) -> Request<Body> {
    let body = serde_json::to_vec(payload).expect("could not serialize payload");

    let mut builder = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json");

    if let Some(key) = key {
        builder = builder.header("Idempotency-Key", key);
    }

    builder
        .body(Body::from(body))
        .expect("failed to build request")
}

fn create_raw_post_request(uri: &str, key: Option<&str>, body: &str) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json");

    if let Some(key) = key {
        builder = builder.header("Idempotency-Key", key);
    }

    builder
        .body(Body::from(body.to_owned()))
        .expect("failed to build raw request")
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

async fn get_status_and_body(app: &Router, request: Request<Body>) -> (StatusCode, Bytes) {
    let response = make_request(app, request).await;
    let status = response.status();
    let body = parse_body(response).await;
    (status, body)
}

async fn get_status_and_order(app: &Router, request: Request<Body>) -> (StatusCode, Option<Order>) {
    let (status, body) = get_status_and_body(app, request).await;
    if let Ok(order) = serde_json::from_slice(&body) {
        (status, Some(order))
    } else {
        (status, None)
    }
}

async fn do_post_request<'a, T: Serialize + ?Sized>(
    uri: &'a str,
    key: Option<&'a str>,
    payload: &'a T,
    app: &'a Router,
) -> (StatusCode, Option<Order>) {
    let request = create_post_request(uri, key, payload);
    get_status_and_order(app, request).await
}

async fn do_post_raw_request(
    uri: &str,
    key: Option<&str>,
    body: &str,
    app: &Router,
) -> (StatusCode, Bytes) {
    let request = create_raw_post_request(uri, key, body);
    get_status_and_body(app, request).await
}

async fn do_get_request(uri: &str, app: &Router) -> (StatusCode, Option<Order>) {
    let request = create_get_request(uri);
    get_status_and_order(app, request).await
}

async fn do_get_body_request(uri: &str, app: &Router) -> (StatusCode, Bytes) {
    let request = create_get_request(uri);
    get_status_and_body(app, request).await
}
