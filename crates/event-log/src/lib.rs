//! Read-only administrative view of the transactional event outbox.

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use boson_admin::AdminPrincipal;
use boson_capability::{Capability, CapabilityDescriptor};
use boson_db::Database;
use boson_events::redact_sensitive_payload;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

#[derive(Clone)]
pub struct EventsCapability {
    database: Option<Database>,
}

impl EventsCapability {
    #[must_use]
    pub fn new(database: Option<Database>) -> Self {
        Self { database }
    }
}

impl Capability for EventsCapability {
    fn descriptor(&self) -> CapabilityDescriptor {
        CapabilityDescriptor {
            name: "events",
            version: env!("CARGO_PKG_VERSION"),
            dependencies: &["admin"],
        }
    }

    fn scopes(&self) -> &'static [&'static str] {
        &["events:read"]
    }

    fn admin_router(&self) -> Router {
        Router::new()
            .route("/events", get(list_events))
            .route("/events/{id}", get(get_event))
            .with_state(self.clone())
    }
}

#[derive(Debug, Serialize)]
struct EventView {
    id: Uuid,
    topic: String,
    payload: Value,
    correlation_id: Option<String>,
    occurred_at: DateTime<Utc>,
    status: String,
    attempts: i32,
    run_at: DateTime<Utc>,
    locked_at: Option<DateTime<Utc>>,
    locked_by: Option<String>,
    last_error: Option<String>,
    dispatched_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct DeliveryView {
    consumer: String,
    status: String,
    attempts: i32,
    last_error: Option<String>,
    first_attempted_at: Option<DateTime<Utc>>,
    last_attempted_at: Option<DateTime<Utc>>,
    delivered_at: Option<DateTime<Utc>>,
}

async fn list_events(
    State(state): State<EventsCapability>,
    Extension(principal): Extension<AdminPrincipal>,
) -> Result<Json<Value>, EventsError> {
    require(&principal)?;
    let rows = sqlx::query(
        "SELECT id, topic, payload, correlation_id, occurred_at, status,
                attempts, run_at, locked_at, locked_by, last_error,
                dispatched_at, created_at
         FROM kernel.outbox ORDER BY created_at DESC LIMIT 250",
    )
    .fetch_all(state.database()?.pool())
    .await
    .map_err(unavailable)?;
    let events = rows
        .iter()
        .map(event_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(json!({ "data": events })))
}

async fn get_event(
    State(state): State<EventsCapability>,
    Extension(principal): Extension<AdminPrincipal>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, EventsError> {
    require(&principal)?;
    let database = state.database()?;
    let row = sqlx::query(
        "SELECT id, topic, payload, correlation_id, occurred_at, status,
                attempts, run_at, locked_at, locked_by, last_error,
                dispatched_at, created_at
         FROM kernel.outbox WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(database.pool())
    .await
    .map_err(unavailable)?
    .ok_or(EventsError::NotFound)?;
    let delivery_rows = sqlx::query(
        "SELECT consumer, status, attempts, last_error, first_attempted_at,
                last_attempted_at, delivered_at
         FROM kernel.event_deliveries
         WHERE event_id = $1 ORDER BY consumer",
    )
    .bind(id)
    .fetch_all(database.pool())
    .await
    .map_err(unavailable)?;
    let deliveries = delivery_rows
        .iter()
        .map(delivery_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(json!({
        "data": {
            "event": event_from_row(&row)?,
            "deliveries": deliveries
        }
    })))
}

fn event_from_row(row: &sqlx::postgres::PgRow) -> Result<EventView, EventsError> {
    Ok(EventView {
        id: row.try_get("id").map_err(unavailable)?,
        topic: row.try_get("topic").map_err(unavailable)?,
        payload: redact_sensitive_payload(&row.try_get("payload").map_err(unavailable)?),
        correlation_id: row.try_get("correlation_id").map_err(unavailable)?,
        occurred_at: row.try_get("occurred_at").map_err(unavailable)?,
        status: row.try_get("status").map_err(unavailable)?,
        attempts: row.try_get("attempts").map_err(unavailable)?,
        run_at: row.try_get("run_at").map_err(unavailable)?,
        locked_at: row.try_get("locked_at").map_err(unavailable)?,
        locked_by: row.try_get("locked_by").map_err(unavailable)?,
        last_error: row.try_get("last_error").map_err(unavailable)?,
        dispatched_at: row.try_get("dispatched_at").map_err(unavailable)?,
        created_at: row.try_get("created_at").map_err(unavailable)?,
    })
}

fn delivery_from_row(row: &sqlx::postgres::PgRow) -> Result<DeliveryView, EventsError> {
    Ok(DeliveryView {
        consumer: row.try_get("consumer").map_err(unavailable)?,
        status: row.try_get("status").map_err(unavailable)?,
        attempts: row.try_get("attempts").map_err(unavailable)?,
        last_error: row.try_get("last_error").map_err(unavailable)?,
        first_attempted_at: row.try_get("first_attempted_at").map_err(unavailable)?,
        last_attempted_at: row.try_get("last_attempted_at").map_err(unavailable)?,
        delivered_at: row.try_get("delivered_at").map_err(unavailable)?,
    })
}

fn require(principal: &AdminPrincipal) -> Result<(), EventsError> {
    if principal.allows("events:read") {
        Ok(())
    } else {
        Err(EventsError::Forbidden)
    }
}

impl EventsCapability {
    fn database(&self) -> Result<&Database, EventsError> {
        self.database
            .as_ref()
            .ok_or_else(|| EventsError::Unavailable("PostgreSQL is disabled".into()))
    }
}

#[derive(Debug)]
enum EventsError {
    Forbidden,
    NotFound,
    Unavailable(String),
}

impl IntoResponse for EventsError {
    fn into_response(self) -> Response {
        let (status, code) = match self {
            Self::Forbidden => (StatusCode::FORBIDDEN, "events.forbidden"),
            Self::NotFound => (StatusCode::NOT_FOUND, "events.not_found"),
            Self::Unavailable(_) => (StatusCode::SERVICE_UNAVAILABLE, "events.unavailable"),
        };
        (
            status,
            Json(json!({
                "error": { "code": code, "message": self.to_string() }
            })),
        )
            .into_response()
    }
}

impl std::fmt::Display for EventsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Forbidden => formatter.write_str("missing required scope `events:read`"),
            Self::NotFound => formatter.write_str("event not found"),
            Self::Unavailable(message) => {
                write!(formatter, "event service unavailable: {message}")
            }
        }
    }
}

#[allow(clippy::needless_pass_by_value)]
fn unavailable(error: impl ToString) -> EventsError {
    EventsError::Unavailable(error.to_string())
}
