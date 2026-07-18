CREATE SCHEMA IF NOT EXISTS todos;

CREATE TABLE IF NOT EXISTS todos.todos (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL,
    title TEXT NOT NULL,
    completed BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    deleted_at TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS todos_owner_id_idx
    ON todos.todos (owner_id)
    WHERE deleted_at IS NULL;
