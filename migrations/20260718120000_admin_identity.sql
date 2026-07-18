CREATE SCHEMA IF NOT EXISTS admin;

CREATE TABLE admin.users (
    id UUID PRIMARY KEY,
    email TEXT NOT NULL,
    display_name TEXT NOT NULL,
    disabled_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT admin_users_email_lowercase CHECK (email = lower(email))
);

CREATE UNIQUE INDEX admin_users_email_unique_idx ON admin.users (email);

CREATE TABLE admin.api_keys (
    id UUID PRIMARY KEY,
    admin_id UUID NOT NULL REFERENCES admin.users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    token_hash TEXT NOT NULL UNIQUE,
    token_prefix TEXT NOT NULL,
    scopes TEXT[] NOT NULL DEFAULT ARRAY[]::TEXT[],
    last_used_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX admin_api_keys_admin_idx ON admin.api_keys (admin_id, created_at DESC);

COMMENT ON SCHEMA admin IS 'Boson platform administrator identities and credentials';
