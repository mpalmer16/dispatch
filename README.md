# dispatch-rs

`dispatch-rs` contains the Rust order service. It exposes the HTTP API for creating and fetching orders and owns the transactional write to both `orders` and `order_outbox`.

In the larger system, this service is the entrypoint for new work:

1. accept an order request
2. enforce idempotency
3. persist the order
4. persist an `order.created` outbox event in the same database transaction

## API

Create an order:

```text
POST /orders
```

Request body:

```json
{
  "customer_id": "customer-123",
  "total_cents": 4999
}
```

Headers:

```text
Content-Type: application/json
Idempotency-Key: optional
```

Responses:

- `201 Created` for a new order
- `200 OK` when the same idempotency key is reused with the same payload
- `409 Conflict` when the same idempotency key is reused with a different payload

Fetch an order:

```text
GET /orders/{id}
```

Health check:

```text
GET /health
```

## What It Writes

On a successful create request, the service writes:

- one row to `orders`
- one row to `order_outbox`

Those writes happen in a single Postgres transaction so the order row and the outbox event stay in sync.

## Local Run

From the parent workspace, start shared infrastructure first:

```bash
cd ..
make local-setup
```

Then run the service from this repo:

```bash
cargo run -p order-service
```

Create an order:

```bash
curl -X POST http://localhost:3000/orders \
  -H "content-type: application/json" \
  -H "Idempotency-Key: example-$(date +%s)" \
  -d '{"customer_id":"customer-1","total_cents":4999}'
```

The service logs when it:

- creates a new order and outbox event
- reuses an existing order for an idempotent retry
- rejects a conflicting reuse of an idempotency key

## Tests

Run the Rust test suite:

```bash
cargo test -p order-service
```

The integration tests use the `dispatch_test` database created by the parent workspace setup.

## How It Fits

This service does not publish to Kafka directly. It writes the outbox row that `dispatch-go` later relays to Kafka for downstream consumers such as `dispatch-kt`.
