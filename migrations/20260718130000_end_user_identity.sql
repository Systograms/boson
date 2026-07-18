CREATE SCHEMA IF NOT EXISTS identity;

CREATE TABLE identity.users (
    id UUID PRIMARY KEY,
    email TEXT NOT NULL,
    display_name TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    email_verified_at TIMESTAMPTZ,
    disabled_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT identity_users_email_lowercase CHECK (email = lower(email))
);

CREATE UNIQUE INDEX identity_users_email_unique_idx ON identity.users (email);

CREATE TABLE identity.sessions (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES identity.users(id) ON DELETE CASCADE,
    refresh_token_hash TEXT NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    last_used_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX identity_sessions_user_idx
    ON identity.sessions (user_id, created_at DESC);
CREATE INDEX identity_sessions_active_idx
    ON identity.sessions (expires_at)
    WHERE revoked_at IS NULL;

COMMENT ON SCHEMA identity IS 'Boson end-user identities and refresh sessions';
