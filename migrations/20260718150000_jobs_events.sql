CREATE TABLE IF NOT EXISTS kernel.jobs (
    id TEXT PRIMARY KEY,
    topic TEXT NOT NULL,
    payload JSONB NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'running', 'completed', 'failed', 'dead')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    max_attempts INTEGER NOT NULL DEFAULT 5 CHECK (max_attempts > 0),
    run_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    locked_at TIMESTAMPTZ,
    locked_by TEXT,
    last_error TEXT,
    correlation_id TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS jobs_lease_candidates_idx
    ON kernel.jobs (run_at, created_at)
    WHERE status IN ('pending', 'running', 'failed');

ALTER TABLE kernel.outbox
    ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'pending',
    ADD COLUMN IF NOT EXISTS attempts INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS run_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    ADD COLUMN IF NOT EXISTS locked_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS locked_by TEXT,
    ADD COLUMN IF NOT EXISTS last_error TEXT;

ALTER TABLE kernel.outbox DROP CONSTRAINT IF EXISTS outbox_status_check;
ALTER TABLE kernel.outbox ADD CONSTRAINT outbox_status_check
    CHECK (status IN ('pending', 'processing', 'dispatched'));

UPDATE kernel.outbox
SET status = 'dispatched'
WHERE dispatched_at IS NOT NULL AND status <> 'dispatched';

CREATE INDEX IF NOT EXISTS outbox_dispatch_candidates_idx
    ON kernel.outbox (run_at, created_at)
    WHERE dispatched_at IS NULL;

CREATE TABLE IF NOT EXISTS kernel.event_deliveries (
    event_id UUID NOT NULL REFERENCES kernel.outbox(id) ON DELETE CASCADE,
    consumer TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'succeeded', 'failed')),
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    first_attempted_at TIMESTAMPTZ,
    last_attempted_at TIMESTAMPTZ,
    delivered_at TIMESTAMPTZ,
    PRIMARY KEY (event_id, consumer)
);

CREATE INDEX IF NOT EXISTS event_deliveries_event_idx
    ON kernel.event_deliveries (event_id, consumer);
