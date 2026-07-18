# Boson Todo Example

Standalone Boson application that proves the public Capability SDK end to end.

## What it demonstrates

- `boson-runtime::Builder::extend` composition for server, worker, and migrate
- Capability-owned migrations under `capabilities/todos/migrations`
- Authenticated app routes (`/v1/todos`)
- Scoped Admin route (`/admin/v1/todos`, `todos:read`)
- Transactional outbox publish via `publish_in_tx`
- Event consumer + health check registration
- Namespaced config under `capabilities.todos`

## Run

From this directory (with Docker available):

```bash
cargo run -p boson-cli --manifest-path ../../Cargo.toml -- migrate
cargo run -p boson-cli --manifest-path ../../Cargo.toml -- dev
```

Or manually:

```bash
docker compose up -d postgres
cargo run -p todo_migrate
cargo run -p todo_server
cargo run -p todo_worker
```

Register a user, create a todo, then inspect `/admin/v1/todos` with the
development Admin token `local-development-token`.
