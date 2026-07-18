CREATE SCHEMA IF NOT EXISTS audit;

CREATE TABLE IF NOT EXISTS audit.entries (
    event_id UUID PRIMARY KEY,
    topic TEXT NOT NULL,
    payload JSONB NOT NULL,
    correlation_id TEXT,
    occurred_at TIMESTAMPTZ NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS audit_entries_occurred_at_idx
    ON audit.entries (occurred_at DESC);

CREATE INDEX IF NOT EXISTS audit_entries_topic_idx
    ON audit.entries (topic);

CREATE INDEX IF NOT EXISTS audit_entries_correlation_idx
    ON audit.entries (correlation_id)
    WHERE correlation_id IS NOT NULL;

COMMENT ON SCHEMA audit IS 'Immutable audit trail built from platform events';
