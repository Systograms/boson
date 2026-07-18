# Boson

Boson is a modular backend platform written in Rust. It combines a production
server, background worker, operational Admin API, dashboard, CLI, and versioned
documentation without coupling application logic to cloud providers.

This repository is the first runnable platform foundation. Identity,
organizations, file storage, jobs, and event inspection are implemented as
capabilities; billing remains out of scope.

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
| `boson-admin` | Platform administrator identities and scoped API keys |
| `boson-database-inspection` | Provider-neutral, read-only database inspection Admin API |
| `boson-identity` | End-user accounts, Argon2id passwords, JWT + refresh sessions |
| `boson-organizations` | Organizations, role memberships, invitations, and authorization |
| `boson-files` | End-user file metadata, uploads/downloads behind the ObjectStore port |
| `boson-storage-local` | Local-filesystem ObjectStore adapter (`adapters/storage-local`) |
| `boson-queue-postgres` | Durable at-least-once PostgreSQL queue adapter |
| `boson-jobs` | Scoped Admin job inspection and manual retry APIs |
| `boson-event-log` | Scoped Admin outbox and delivery inspection APIs |

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
and serves the dashboard. Uploaded files persist in the `boson-storage` named
volume, mounted at `/var/lib/boson/storage` in the server and worker.

## APIs currently available

Public:

- `GET /`
- `GET /healthz`
- `GET /readyz`

End-user identity (requires PostgreSQL and `auth.jwt_secret`):

- `POST /v1/auth/register`, `POST /v1/auth/login` — return the user, an access
  token, and a rotating refresh token
- `POST /v1/auth/refresh` — rotates the refresh token transactionally
- `POST /v1/auth/logout` — revokes the refresh session
- `GET /v1/auth/me` — requires a Bearer access token

Files (requires a Bearer access token, PostgreSQL, and configured storage):

- `POST /v1/files` — direct upload; raw bytes with required `Content-Type` and
  `X-Boson-Filename` headers
- `GET /v1/files` — lists the caller's files (metadata only)
- `GET /v1/files/{id}/content` — downloads the file bytes
- `DELETE /v1/files/{id}` — soft-deletes metadata and removes the bytes

Organizations (requires a Bearer access token and PostgreSQL):

- `POST /v1/organizations`, `GET /v1/organizations`
- `GET /v1/organizations/{id}`, `PATCH /v1/organizations/{id}`,
  `DELETE /v1/organizations/{id}`
- `GET /v1/organizations/{id}/members`
- `PATCH /v1/organizations/{id}/members/{user_id}`,
  `DELETE /v1/organizations/{id}/members/{user_id}`
- `POST /v1/organizations/{id}/invitations` — returns the opaque invitation
  token once; only its SHA-256 hash is stored
- `POST /v1/organization-invitations/accept` — validates the authenticated
  user's email and atomically consumes the invitation

Admin (Bearer token required):

- `GET /admin/v1/health`
- `GET /admin/v1/overview`
- `GET /admin/v1/requests`
- `GET /admin/v1/config` — effective configuration with secrets redacted
- `GET /admin/v1/users`, `GET /admin/v1/sessions` — end-user directory
  (`identity:read` scope)
- `GET /admin/v1/organizations`, `GET /admin/v1/organization-memberships`,
  `GET /admin/v1/organization-invitations` (`organizations:read` scope)
- `GET /admin/v1/files` — file metadata across all users, no storage paths
  (`storage:read` scope)
- `GET /admin/v1/jobs` and `POST /admin/v1/jobs/{id}/retry`
  (`jobs:read` / `jobs:write` scopes)
- `GET /admin/v1/events` and `GET /admin/v1/events/{id}`
  (`events:read` scope)
- `GET /admin/v1/database`, `/database/tables`, and table schema/row routes
  (`database:read` scope; read-only, paginated, and redacted)

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
export BOSON__STORAGE__LOCAL_ROOT=data/storage
```

Database inspection is disabled by default. Enable
`database_inspection.enabled` explicitly and use
`database_inspection.allowed_namespaces` as a production allowlist. Row values
for configured `redacted_columns` are never selected from the provider.

Object storage is selected by `storage.provider` at the composition root.
Only `local` is supported today; any other value fails startup.

The background queue is configured under `queue` and currently requires the
`postgres` provider. Workers lease due jobs with `SKIP LOCKED`; expired leases
are visible again, handler failures back off until `max_attempts`, and the final
failure is retained as `dead`. Outbox events use the same lease discipline.
Per-consumer deliveries are idempotently recorded, and an event is dispatched
only after every matching registered consumer succeeds. Topics with no
registered consumer are still marked dispatched and remain inspectable.

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
