CREATE SCHEMA IF NOT EXISTS kernel;
CREATE SCHEMA IF NOT EXISTS ops;

CREATE TABLE IF NOT EXISTS kernel.outbox (
    id UUID PRIMARY KEY,
    topic TEXT NOT NULL,
    payload JSONB NOT NULL,
    correlation_id TEXT,
    occurred_at TIMESTAMPTZ NOT NULL,
    dispatched_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS outbox_pending_idx
    ON kernel.outbox (created_at)
    WHERE dispatched_at IS NULL;

CREATE TABLE IF NOT EXISTS ops.worker_heartbeats (
    name TEXT PRIMARY KEY,
    last_heartbeat TIMESTAMPTZ NOT NULL
);

COMMENT ON SCHEMA kernel IS 'Boson platform-owned infrastructure tables';
COMMENT ON SCHEMA ops IS 'Boson operational read models';
