CREATE SCHEMA IF NOT EXISTS items;

CREATE TABLE IF NOT EXISTS items.items (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL,
    title TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS items_owner_id_idx
    ON items.items (owner_id)
    WHERE deleted_at IS NULL;
