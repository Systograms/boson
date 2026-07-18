CREATE SCHEMA IF NOT EXISTS files;

CREATE TABLE files.files (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES identity.users(id) ON DELETE CASCADE,
    object_key TEXT NOT NULL UNIQUE,
    filename TEXT NOT NULL,
    content_type TEXT NOT NULL,
    size_bytes BIGINT NOT NULL CHECK (size_bytes >= 0),
    status TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'ready')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ
);

CREATE INDEX files_files_owner_idx
    ON files.files (owner_id, created_at DESC)
    WHERE deleted_at IS NULL;

COMMENT ON SCHEMA files IS 'Boson end-user file metadata; bytes live in the object store';
