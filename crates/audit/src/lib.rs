//! Immutable audit trail built from the platform event stream.
//!
//! The audit capability never receives direct writes from other capabilities.
//! It subscribes to every outbox event through a wildcard consumer and copies
//! each event into `audit.entries`, so the trail is exactly as trustworthy as
//! the transactional outbox itself. Entries are append-only; there is no
//! update or delete path.

use std::sync::Arc;

use async_trait::async_trait;
use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use boson_admin::AdminPrincipal;
use boson_capability::{Capability, CapabilityDescriptor};
use boson_db::Database;
use boson_events::{EventConsumer, EventEnvelope, EventError, redact_sensitive_payload};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::Row;
use thiserror::Error;
use uuid::Uuid;

const MAX_PAGE_SIZE: i64 = 250;

#[derive(Clone)]
pub struct AuditCapability {
    database: Option<Database>,
}

impl AuditCapability {
    #[must_use]
    pub fn new(database: Option<Database>) -> Self {
        Self { database }
    }

    fn database(&self) -> Result<&Database, AuditError> {
        self.database
            .as_ref()
            .ok_or_else(|| AuditError::Unavailable("PostgreSQL is disabled".into()))
    }
}

impl Capability for AuditCapability {
    fn descriptor(&self) -> CapabilityDescriptor {
        CapabilityDescriptor {
            name: "audit",
            version: env!("CARGO_PKG_VERSION"),
            dependencies: &["admin"],
        }
    }

    fn admin_router(&self) -> Router {
        Router::new()
            .route("/audit", get(list_entries))
            .with_state(self.clone())
    }

    fn event_consumers(&self) -> Vec<Arc<dyn EventConsumer>> {
        self.database.as_ref().map_or_else(Vec::new, |database| {
            vec![Arc::new(AuditRecorder {
                database: database.clone(),
            }) as Arc<dyn EventConsumer>]
        })
    }
}

/// Wildcard consumer that copies every event into the audit trail.
struct AuditRecorder {
    database: Database,
}

#[async_trait]
impl EventConsumer for AuditRecorder {
    fn name(&self) -> &'static str {
        "audit.recorder"
    }

    fn topic(&self) -> &'static str {
        "*"
    }

    async fn handle(&self, event: &EventEnvelope) -> Result<(), EventError> {
        sqlx::query(
            "INSERT INTO audit.entries
             (event_id, topic, payload, correlation_id, occurred_at)
             VALUES ($1, $2, $3, $4, $5)
             ON CONFLICT (event_id) DO NOTHING",
        )
        .bind(event.id)
        .bind(&event.topic)
        .bind(redact_sensitive_payload(&event.payload))
        .bind(&event.correlation_id)
        .bind(event.occurred_at)
        .execute(self.database.pool())
        .await
        .map_err(|error| EventError::Consumer(error.to_string()))?;
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct AuditEntry {
    event_id: Uuid,
    topic: String,
    payload: Value,
    correlation_id: Option<String>,
    occurred_at: DateTime<Utc>,
    recorded_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct ListQuery {
    topic: Option<String>,
    limit: Option<i64>,
}

async fn list_entries(
    State(state): State<AuditCapability>,
    Extension(principal): Extension<AdminPrincipal>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Value>, AuditError> {
    if !principal.allows("audit:read") {
        return Err(AuditError::Forbidden);
    }
    let database = state.database()?;
    let limit = query.limit.unwrap_or(MAX_PAGE_SIZE).clamp(1, MAX_PAGE_SIZE);
    let rows = match &query.topic {
        Some(topic) => {
            sqlx::query(
                "SELECT event_id, topic, payload, correlation_id, occurred_at, recorded_at
                 FROM audit.entries
                 WHERE topic = $1
                 ORDER BY occurred_at DESC
                 LIMIT $2",
            )
            .bind(topic)
            .bind(limit)
            .fetch_all(database.pool())
            .await
        }
        None => {
            sqlx::query(
                "SELECT event_id, topic, payload, correlation_id, occurred_at, recorded_at
                 FROM audit.entries
                 ORDER BY occurred_at DESC
                 LIMIT $1",
            )
            .bind(limit)
            .fetch_all(database.pool())
            .await
        }
    }
    .map_err(unavailable)?;
    let entries = rows
        .iter()
        .map(entry_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(json!({ "data": entries })))
}

fn entry_from_row(row: &sqlx::postgres::PgRow) -> Result<AuditEntry, AuditError> {
    Ok(AuditEntry {
        event_id: row.try_get("event_id").map_err(unavailable)?,
        topic: row.try_get("topic").map_err(unavailable)?,
        payload: row.try_get("payload").map_err(unavailable)?,
        correlation_id: row.try_get("correlation_id").map_err(unavailable)?,
        occurred_at: row.try_get("occurred_at").map_err(unavailable)?,
        recorded_at: row.try_get("recorded_at").map_err(unavailable)?,
    })
}

#[allow(clippy::needless_pass_by_value)]
fn unavailable(error: impl ToString) -> AuditError {
    AuditError::Unavailable(error.to_string())
}

#[derive(Debug, Error)]
enum AuditError {
    #[error("missing required scope `audit:read`")]
    Forbidden,
    #[error("audit service unavailable: {0}")]
    Unavailable(String),
}

impl IntoResponse for AuditError {
    fn into_response(self) -> Response {
        let (status, code) = match self {
            Self::Forbidden => (StatusCode::FORBIDDEN, "audit.forbidden"),
            Self::Unavailable(_) => (StatusCode::SERVICE_UNAVAILABLE, "audit.unavailable"),
        };
        (
            status,
            Json(json!({
                "error": {
                    "code": code,
                    "message": self.to_string()
                }
            })),
        )
            .into_response()
    }
}
