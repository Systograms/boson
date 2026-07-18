//! Operational read models shared by the Server, Worker, and Admin API.
//!
//! Request traces are persisted to PostgreSQL when a database is available so
//! the Admin API and dashboard survive process restarts. An in-memory ring
//! buffer remains as a fallback for local runs without Postgres.

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use boson_capability::{Capability, CapabilityDescriptor};
use boson_db::Database;
use boson_kernel::PlatformConfig;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::Row;
use tokio::sync::RwLock;
use uuid::Uuid;

const TRACE_CAPACITY: usize = 500;
const TRACE_LIST_LIMIT: i64 = 500;

#[derive(Debug, Clone, Serialize)]
pub struct RequestTrace {
    pub request_id: String,
    pub started_at: DateTime<Utc>,
    pub method: String,
    pub path: String,
    pub status_code: u16,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkerStatus {
    pub name: String,
    pub last_heartbeat: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct Overview {
    pub total_requests: u64,
    pub total_errors: u64,
    pub error_rate: f64,
    pub retained_traces: usize,
}

#[derive(Debug, Serialize)]
struct CorrelatedEvent {
    id: Uuid,
    topic: String,
    status: String,
    attempts: i32,
    occurred_at: DateTime<Utc>,
    dispatched_at: Option<DateTime<Utc>>,
    last_error: Option<String>,
}

#[derive(Debug, Serialize)]
struct CorrelatedJob {
    id: String,
    topic: String,
    status: String,
    attempts: i32,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    last_error: Option<String>,
}

#[derive(Clone)]
pub struct OpsState {
    database: Option<Database>,
    requests: Arc<AtomicU64>,
    errors: Arc<AtomicU64>,
    traces: Arc<RwLock<VecDeque<RequestTrace>>>,
    workers: Arc<RwLock<Vec<WorkerStatus>>>,
}

impl OpsState {
    #[must_use]
    pub fn new(database: Option<Database>) -> Self {
        Self {
            database,
            requests: Arc::new(AtomicU64::new(0)),
            errors: Arc::new(AtomicU64::new(0)),
            traces: Arc::new(RwLock::new(VecDeque::with_capacity(TRACE_CAPACITY))),
            workers: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn record(&self, trace: RequestTrace) {
        self.requests.fetch_add(1, Ordering::Relaxed);
        if trace.status_code >= 500 {
            self.errors.fetch_add(1, Ordering::Relaxed);
        }

        if let Some(database) = &self.database {
            if let Err(error) = persist_trace(database, &trace).await {
                tracing::warn!(%error, request_id = %trace.request_id, "failed to persist request trace");
                self.push_memory(trace).await;
            }
            return;
        }
        self.push_memory(trace).await;
    }

    async fn push_memory(&self, trace: RequestTrace) {
        let mut traces = self.traces.write().await;
        if traces.len() == TRACE_CAPACITY {
            traces.pop_front();
        }
        traces.push_back(trace);
    }

    pub async fn traces(&self) -> Vec<RequestTrace> {
        if let Some(database) = &self.database {
            match list_traces(database, TRACE_LIST_LIMIT).await {
                Ok(traces) => return traces,
                Err(error) => {
                    tracing::warn!(%error, "failed to list persisted request traces");
                }
            }
        }
        self.traces.read().await.iter().rev().cloned().collect()
    }

    pub async fn trace(&self, request_id: &str) -> Option<RequestTrace> {
        if let Some(database) = &self.database {
            match get_trace(database, request_id).await {
                Ok(trace) => return trace,
                Err(error) => {
                    tracing::warn!(%error, %request_id, "failed to load request trace");
                }
            }
        }
        self.traces
            .read()
            .await
            .iter()
            .rev()
            .find(|trace| trace.request_id == request_id)
            .cloned()
    }

    pub async fn heartbeat(&self, name: impl Into<String>) {
        let name = name.into();
        let mut workers = self.workers.write().await;
        if let Some(worker) = workers.iter_mut().find(|worker| worker.name == name) {
            worker.last_heartbeat = Utc::now();
        } else {
            workers.push(WorkerStatus {
                name,
                last_heartbeat: Utc::now(),
            });
        }
    }

    pub async fn workers(&self) -> Vec<WorkerStatus> {
        self.workers.read().await.clone()
    }

    pub async fn overview(&self) -> Overview {
        if let Some(database) = &self.database
            && let Ok(overview) = overview_from_db(database).await
        {
            return overview;
        }
        let total_requests = self.requests.load(Ordering::Relaxed);
        let total_errors = self.errors.load(Ordering::Relaxed);
        Overview {
            total_requests,
            total_errors,
            error_rate: if total_requests == 0 {
                0.0
            } else {
                total_errors as f64 / total_requests as f64
            },
            retained_traces: self.traces.read().await.len(),
        }
    }
}

impl Default for OpsState {
    fn default() -> Self {
        Self::new(None)
    }
}

#[derive(Clone)]
struct OpsCapabilityState {
    config: Arc<PlatformConfig>,
    database: Option<Database>,
    ops: OpsState,
}

#[derive(Clone)]
pub struct OpsCapability {
    state: OpsCapabilityState,
}

impl OpsCapability {
    #[must_use]
    pub fn new(config: Arc<PlatformConfig>, database: Option<Database>, ops: OpsState) -> Self {
        Self {
            state: OpsCapabilityState {
                config,
                database,
                ops,
            },
        }
    }
}

impl Capability for OpsCapability {
    fn descriptor(&self) -> CapabilityDescriptor {
        CapabilityDescriptor {
            name: "ops",
            version: env!("CARGO_PKG_VERSION"),
            dependencies: &[],
        }
    }

    fn admin_router(&self) -> Router {
        Router::new()
            .route("/health", get(admin_health))
            .route("/overview", get(admin_overview))
            .route("/requests", get(admin_requests))
            .route("/requests/{request_id}", get(admin_request_detail))
            .route("/config", get(admin_config))
            .with_state(self.state.clone())
    }
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
    checks: Vec<DependencyHealth>,
}

#[derive(Serialize)]
struct DependencyHealth {
    name: &'static str,
    status: &'static str,
    message: Option<String>,
}

async fn admin_health(State(state): State<OpsCapabilityState>) -> Json<HealthResponse> {
    let database = match &state.database {
        Some(database) => match database.ping().await {
            Ok(()) => DependencyHealth {
                name: "postgres",
                status: "ok",
                message: None,
            },
            Err(error) => DependencyHealth {
                name: "postgres",
                status: "down",
                message: Some(error.to_string()),
            },
        },
        None => DependencyHealth {
            name: "postgres",
            status: "disabled",
            message: Some("set database.connect_on_boot=true to enable".into()),
        },
    };
    let worker = match &state.database {
        Some(database) => match database.worker_heartbeats().await {
            Ok(workers)
                if workers
                    .iter()
                    .any(|worker| (Utc::now() - worker.last_heartbeat).num_seconds() < 30) =>
            {
                DependencyHealth {
                    name: "worker",
                    status: "ok",
                    message: None,
                }
            }
            Ok(_) => DependencyHealth {
                name: "worker",
                status: "down",
                message: Some("no recent worker heartbeat".into()),
            },
            Err(error) => DependencyHealth {
                name: "worker",
                status: "down",
                message: Some(error.to_string()),
            },
        },
        None => DependencyHealth {
            name: "worker",
            status: "disabled",
            message: Some("worker health requires PostgreSQL".into()),
        },
    };
    let status = if database.status == "down" || worker.status == "down" {
        "degraded"
    } else {
        "ok"
    };
    Json(HealthResponse {
        status,
        version: env!("CARGO_PKG_VERSION"),
        checks: vec![database, worker],
    })
}

async fn admin_overview(State(state): State<OpsCapabilityState>) -> Json<Value> {
    let workers = match &state.database {
        Some(database) => database
            .worker_heartbeats()
            .await
            .map_or_else(|_| json!([]), |workers| json!(workers)),
        None => json!(state.ops.workers().await),
    };
    Json(json!({
        "metrics": state.ops.overview().await,
        "workers": workers
    }))
}

async fn admin_requests(State(state): State<OpsCapabilityState>) -> Json<Value> {
    Json(json!({ "data": state.ops.traces().await }))
}

async fn admin_request_detail(
    State(state): State<OpsCapabilityState>,
    Path(request_id): Path<String>,
) -> Result<Json<Value>, OpsError> {
    let Some(request) = state.ops.trace(&request_id).await else {
        return Err(OpsError::NotFound);
    };
    let (events, jobs) = match &state.database {
        Some(database) => (
            correlated_events(database, &request_id)
                .await
                .map_err(OpsError::Unavailable)?,
            correlated_jobs(database, &request_id)
                .await
                .map_err(OpsError::Unavailable)?,
        ),
        None => (Vec::new(), Vec::new()),
    };
    Ok(Json(json!({
        "data": {
            "request": request,
            "events": events,
            "jobs": jobs
        }
    })))
}

async fn admin_config(State(state): State<OpsCapabilityState>) -> Json<Value> {
    Json(json!({
        "snapshot_id": state.config.snapshot_id(),
        "effective": state.config.redacted(),
        "read_only": true
    }))
}

#[derive(Debug)]
enum OpsError {
    NotFound,
    Unavailable(String),
}

impl IntoResponse for OpsError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                "ops.not_found",
                "request trace not found".to_owned(),
            ),
            Self::Unavailable(message) => {
                (StatusCode::SERVICE_UNAVAILABLE, "ops.unavailable", message)
            }
        };
        (
            status,
            Json(json!({
                "error": {
                    "code": code,
                    "message": message
                }
            })),
        )
            .into_response()
    }
}

async fn persist_trace(database: &Database, trace: &RequestTrace) -> Result<(), sqlx::Error> {
    let request_id =
        Uuid::parse_str(&trace.request_id).map_err(|error| sqlx::Error::Decode(Box::new(error)))?;
    let status_code = i16::try_from(trace.status_code).unwrap_or(i16::MAX);
    let duration_ms = i64::try_from(trace.duration_ms).unwrap_or(i64::MAX);
    sqlx::query(
        "INSERT INTO ops.request_traces
         (request_id, started_at, method, path, status_code, duration_ms)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (request_id) DO NOTHING",
    )
    .bind(request_id)
    .bind(trace.started_at)
    .bind(&trace.method)
    .bind(&trace.path)
    .bind(status_code)
    .bind(duration_ms)
    .execute(database.pool())
    .await?;
    Ok(())
}

async fn list_traces(database: &Database, limit: i64) -> Result<Vec<RequestTrace>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT request_id, started_at, method, path, status_code, duration_ms
         FROM ops.request_traces
         ORDER BY started_at DESC
         LIMIT $1",
    )
    .bind(limit)
    .fetch_all(database.pool())
    .await?;
    rows.iter().map(trace_from_row).collect()
}

async fn get_trace(
    database: &Database,
    request_id: &str,
) -> Result<Option<RequestTrace>, sqlx::Error> {
    let Ok(request_id) = Uuid::parse_str(request_id) else {
        return Ok(None);
    };
    let row = sqlx::query(
        "SELECT request_id, started_at, method, path, status_code, duration_ms
         FROM ops.request_traces
         WHERE request_id = $1",
    )
    .bind(request_id)
    .fetch_optional(database.pool())
    .await?;
    row.as_ref().map(trace_from_row).transpose()
}

async fn overview_from_db(database: &Database) -> Result<Overview, sqlx::Error> {
    let row = sqlx::query(
        "SELECT
            COUNT(*)::BIGINT AS total_requests,
            COUNT(*) FILTER (WHERE status_code >= 500)::BIGINT AS total_errors
         FROM ops.request_traces",
    )
    .fetch_one(database.pool())
    .await?;
    let total_requests = u64::try_from(row.try_get::<i64, _>("total_requests")?).unwrap_or(0);
    let total_errors = u64::try_from(row.try_get::<i64, _>("total_errors")?).unwrap_or(0);
    Ok(Overview {
        total_requests,
        total_errors,
        error_rate: if total_requests == 0 {
            0.0
        } else {
            total_errors as f64 / total_requests as f64
        },
        retained_traces: usize::try_from(total_requests).unwrap_or(usize::MAX),
    })
}

async fn correlated_events(
    database: &Database,
    request_id: &str,
) -> Result<Vec<CorrelatedEvent>, String> {
    let rows = sqlx::query(
        "SELECT id, topic, status, attempts, occurred_at, dispatched_at, last_error
         FROM kernel.outbox
         WHERE correlation_id = $1
         ORDER BY occurred_at ASC",
    )
    .bind(request_id)
    .fetch_all(database.pool())
    .await
    .map_err(|error| error.to_string())?;
    rows.into_iter()
        .map(|row| {
            Ok(CorrelatedEvent {
                id: row.try_get("id").map_err(|error| error.to_string())?,
                topic: row.try_get("topic").map_err(|error| error.to_string())?,
                status: row.try_get("status").map_err(|error| error.to_string())?,
                attempts: row.try_get("attempts").map_err(|error| error.to_string())?,
                occurred_at: row
                    .try_get("occurred_at")
                    .map_err(|error| error.to_string())?,
                dispatched_at: row
                    .try_get("dispatched_at")
                    .map_err(|error| error.to_string())?,
                last_error: row
                    .try_get("last_error")
                    .map_err(|error| error.to_string())?,
            })
        })
        .collect()
}

async fn correlated_jobs(
    database: &Database,
    request_id: &str,
) -> Result<Vec<CorrelatedJob>, String> {
    let rows = sqlx::query(
        "SELECT id, topic, status, attempts, created_at, updated_at, last_error
         FROM kernel.jobs
         WHERE correlation_id = $1
         ORDER BY created_at ASC",
    )
    .bind(request_id)
    .fetch_all(database.pool())
    .await
    .map_err(|error| error.to_string())?;
    rows.into_iter()
        .map(|row| {
            Ok(CorrelatedJob {
                id: row.try_get("id").map_err(|error| error.to_string())?,
                topic: row.try_get("topic").map_err(|error| error.to_string())?,
                status: row.try_get("status").map_err(|error| error.to_string())?,
                attempts: row.try_get("attempts").map_err(|error| error.to_string())?,
                created_at: row
                    .try_get("created_at")
                    .map_err(|error| error.to_string())?,
                updated_at: row
                    .try_get("updated_at")
                    .map_err(|error| error.to_string())?,
                last_error: row
                    .try_get("last_error")
                    .map_err(|error| error.to_string())?,
            })
        })
        .collect()
}

fn trace_from_row(row: &sqlx::postgres::PgRow) -> Result<RequestTrace, sqlx::Error> {
    let request_id: Uuid = row.try_get("request_id")?;
    let status_code: i16 = row.try_get("status_code")?;
    let duration_ms: i64 = row.try_get("duration_ms")?;
    Ok(RequestTrace {
        request_id: request_id.to_string(),
        started_at: row.try_get("started_at")?,
        method: row.try_get("method")?,
        path: row.try_get("path")?,
        status_code: u16::try_from(status_code).unwrap_or(0),
        duration_ms: u64::try_from(duration_ms).unwrap_or(0),
    })
}
