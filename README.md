# Boson

Boson is a modular backend platform written in Rust. It combines a production
server, background worker, operational Admin API, dashboard, CLI, and versioned
documentation without coupling application logic to cloud providers.

This repository is the first runnable platform foundation. It deliberately does
not pretend unfinished capabilities such as identity, organizations, or billing
are production-ready.

## Applications

| Application | Purpose |
|---|---|
| `apps/server` | HTTP host for public and Admin APIs |
| `apps/worker` | Background processing and worker heartbeats |
| `apps/dashboard` | Optional React client of the Admin API |
| `apps/cli` | Developer and operator client |
| `apps/docs` | Versioned documentation site |

## Platform crates

| Crate | Stable responsibility |
|---|---|
| `boson-kernel` | Typed configuration, redaction, context, telemetry |
| `boson-capability` | Capability registration, dependency order, routes, jobs |
| `boson-ports` | Provider-agnostic storage, queue, mailer, health contracts |
| `boson-events` | Versioned event envelope and consumer contract |
| `boson-db` | PostgreSQL pool, migrations, outbox, worker heartbeat |
| `boson-ops` | Request traces, overview metrics, worker state |

Dependency direction:

```text
apps → capabilities → kernel / ports / events
apps → adapters → vendor SDKs

dashboard / CLI → Admin API → server → providers
```

Provider SDK types must not appear in capabilities or public contracts.

## Run the foundation locally

### Without PostgreSQL

The local config disables database startup, so the server can be explored
immediately:

```bash
cargo run -p boson-server
```

Then:

```bash
curl http://localhost:8080/healthz
cargo run -p boson-cli -- doctor
cargo run -p boson-cli -- \
  --admin-token local-development-token config
```

Run the web applications:

```bash
npm run dev --prefix apps/dashboard
npm run dev --prefix apps/docs
```

### Complete local stack

```bash
docker compose up --build
```

- Server: <http://localhost:8080>
- Dashboard: <http://localhost:3000>
- PostgreSQL: `localhost:5432`
- Development Admin token: `local-development-token`

The compose stack enables PostgreSQL, applies migrations, starts the worker,
and serves the dashboard.

## APIs currently available

Public:

- `GET /`
- `GET /healthz`
- `GET /readyz`

Admin (Bearer token required):

- `GET /admin/v1/health`
- `GET /admin/v1/overview`
- `GET /admin/v1/requests`
- `GET /admin/v1/config` — effective configuration with secrets redacted

## Configuration

Configuration is loaded in this order:

1. Typed defaults
2. Optional YAML file (`BOSON_CONFIG`, default `config/local.yaml`)
3. Environment variables prefixed with `BOSON__`

Example:

```bash
export BOSON__DATABASE__CONNECT_ON_BOOT=true
export BOSON__DATABASE__URL=postgres://boson:boson@localhost:5432/boson
export BOSON__ADMIN__BOOTSTRAP_TOKEN=replace-me
```

The Admin API only exposes a redacted snapshot. Never commit production secrets.

## Quality checks

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm run build --prefix apps/dashboard
npm run build --prefix apps/docs
```

## Architectural locks

1. PostgreSQL is the system of record.
2. Server and Worker never depend on Dashboard, CLI, or Docs.
3. Dashboard and CLI only use APIs.
4. Admin and application authorization remain separate.
5. Capabilities own their schema and do not query another capability's tables.
6. Side effects use versioned events and a transactional outbox.
7. Provider implementations remain behind small ports.
8. Deployment packaging never leaks into application logic.

## Next vertical slice

The next product slice should be Admin bootstrap identity followed by end-user
identity. It should add real capability registration, migrations, App/Admin
routes, audit events, and integration tests without widening the kernel.
