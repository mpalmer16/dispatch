CREATE EXTENSION IF NOT EXISTS pgcrypto;

CREATE TABLE dispatch_jobs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    order_id UUID NOT NULL UNIQUE,
    customer_id TEXT NOT NULL,
    status TEXT NOT NULL,
    region TEXT NOT NULL,
    priority TEXT NOT NULL,
    source_event_id UUID NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
