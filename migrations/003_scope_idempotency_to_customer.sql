ALTER TABLE orders
DROP CONSTRAINT orders_idempotency_key_key;

ALTER TABLE orders
ADD CONSTRAINT orders_customer_id_idempotency_key_key
UNIQUE (customer_id, idempotency_key);
