CREATE SCHEMA IF NOT EXISTS notifications;

CREATE TABLE IF NOT EXISTS notifications.deliveries (
    event_id UUID PRIMARY KEY,
    kind TEXT NOT NULL,
    recipient TEXT NOT NULL,
    subject TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'sent', 'failed')),
    attempts INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_attempted_at TIMESTAMPTZ,
    sent_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS notification_deliveries_created_idx
    ON notifications.deliveries (created_at DESC);
CREATE INDEX IF NOT EXISTS notification_deliveries_status_idx
    ON notifications.deliveries (status, created_at DESC);

CREATE TABLE IF NOT EXISTS identity.email_action_tokens (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES identity.users(id) ON DELETE CASCADE,
    purpose TEXT NOT NULL CHECK (purpose IN ('verify_email', 'reset_password')),
    token_hash TEXT NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS identity_email_actions_active_idx
    ON identity.email_action_tokens (user_id, purpose, expires_at)
    WHERE consumed_at IS NULL;

COMMENT ON SCHEMA notifications IS
    'Durable delivery history for provider-agnostic platform notifications';
