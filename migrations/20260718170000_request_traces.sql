CREATE TABLE IF NOT EXISTS ops.request_traces (
    request_id UUID PRIMARY KEY,
    started_at TIMESTAMPTZ NOT NULL,
    method TEXT NOT NULL,
    path TEXT NOT NULL,
    status_code SMALLINT NOT NULL CHECK (status_code >= 100 AND status_code < 600),
    duration_ms BIGINT NOT NULL CHECK (duration_ms >= 0),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS request_traces_started_at_idx
    ON ops.request_traces (started_at DESC);

CREATE INDEX IF NOT EXISTS request_traces_path_idx
    ON ops.request_traces (path);

CREATE INDEX IF NOT EXISTS request_traces_status_idx
    ON ops.request_traces (status_code);

CREATE INDEX IF NOT EXISTS outbox_correlation_id_idx
    ON kernel.outbox (correlation_id)
    WHERE correlation_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS jobs_correlation_id_idx
    ON kernel.jobs (correlation_id)
    WHERE correlation_id IS NOT NULL;

COMMENT ON TABLE ops.request_traces IS
    'Durable HTTP request traces for Admin API / dashboard correlation';
