# Architecture

## Product boundary

Boson consists of five independently usable applications:

- **Server** owns public APIs, Admin APIs, and platform composition.
- **Worker** owns background execution and never serves user traffic.
- **Dashboard** is an optional Admin API client.
- **CLI** is a developer/operator API client plus local tooling.
- **Docs** is static and versioned with releases.

## Dependency rule

```text
Host applications
    ↓
Capabilities (domain behavior)
    ↓
Kernel + Ports + Events
    ↑
Adapters (provider implementations)
```

The composition root in Server and Worker is the only place where concrete
adapters are selected. Domain crates cannot depend on cloud SDKs.

## Runtime lifecycle

1. Merge typed configuration sources.
2. Install telemetry before doing work.
3. Construct PostgreSQL and provider adapters.
4. Apply migrations when explicitly enabled.
5. Register capabilities, routes, consumers, jobs, and health checks.
6. Start the HTTP host or worker loops.
7. Drain work and flush telemetry during graceful shutdown.

## API boundary

- `/v1/*` is the future application API authenticated as an end user.
- `/admin/v1/*` is the platform contract authenticated as an operator.
- `/healthz` and `/readyz` support orchestrator probes.

An end-user credential must never authorize an Admin route.

## Data ownership

PostgreSQL is the sole database target. Each capability owns a schema and its
migrations. Other capabilities reference IDs and use public services or
versioned events; cross-schema reads are forbidden.

## Events and jobs

Events are immutable facts named `{capability}.{entity}_{verb}.vN`. They are
written to the outbox in the same transaction as domain changes. Delivery is
at-least-once, so consumers must be idempotent.

Jobs are imperative work units. Workers lease, acknowledge, retry, and dead
letter them through the Queue port. Schedules only enqueue jobs; they do not
execute business behavior inline.

## Stability

Stable surfaces are Admin/App OpenAPI, port contracts, configuration keys,
event/job schemas, middleware slots, and capability registration. Internal
Axum, SQLx, and provider types are not platform contracts.
