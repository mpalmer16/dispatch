ALTER TABLE orders
ADD COLUMN idempotency_key TEXT UNIQUE;
