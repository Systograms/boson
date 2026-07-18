//! Administrative job inspection and manual retry capability.

use std::sync::Arc;

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use boson_admin::AdminPrincipal;
use boson_capability::{Capability, CapabilityDescriptor};
use boson_ports::{HealthCheck, PortError, Queue};
use serde_json::{Value, json};

#[derive(Clone)]
pub struct JobsCapability {
    queue: Option<Arc<dyn Queue>>,
}

impl JobsCapability {
    #[must_use]
    pub fn new(queue: Option<Arc<dyn Queue>>) -> Self {
        Self { queue }
    }
}

impl Capability for JobsCapability {
    fn descriptor(&self) -> CapabilityDescriptor {
        CapabilityDescriptor {
            name: "jobs",
            version: env!("CARGO_PKG_VERSION"),
            dependencies: &["admin"],
        }
    }

    fn admin_router(&self) -> Router {
        Router::new()
            .route("/jobs", get(list_jobs))
            .route("/jobs/{id}/retry", post(retry_job))
            .with_state(self.clone())
    }

    fn health_checks(&self) -> Vec<Arc<dyn HealthCheck>> {
        self.queue.as_ref().map_or_else(Vec::new, |queue| {
            vec![Arc::clone(queue) as Arc<dyn HealthCheck>]
        })
    }
}

async fn list_jobs(
    State(state): State<JobsCapability>,
    Extension(principal): Extension<AdminPrincipal>,
) -> Result<Json<Value>, JobsError> {
    require(&principal, "jobs:read")?;
    let jobs = state.queue()?.list(250).await.map_err(JobsError::from)?;
    Ok(Json(json!({ "data": jobs })))
}

async fn retry_job(
    State(state): State<JobsCapability>,
    Extension(principal): Extension<AdminPrincipal>,
    Path(id): Path<String>,
) -> Result<Json<Value>, JobsError> {
    require(&principal, "jobs:write")?;
    state.queue()?.requeue(&id).await.map_err(JobsError::from)?;
    Ok(Json(json!({ "data": { "id": id, "status": "pending" } })))
}

fn require(principal: &AdminPrincipal, scope: &'static str) -> Result<(), JobsError> {
    if principal.allows(scope) {
        Ok(())
    } else {
        Err(JobsError::Forbidden(scope))
    }
}

#[derive(Debug)]
enum JobsError {
    Forbidden(&'static str),
    NotFound,
    Unavailable(String),
}

impl From<PortError> for JobsError {
    fn from(error: PortError) -> Self {
        match error {
            PortError::NotFound => Self::NotFound,
            other => Self::Unavailable(other.to_string()),
        }
    }
}

impl JobsCapability {
    fn queue(&self) -> Result<&Arc<dyn Queue>, JobsError> {
        self.queue
            .as_ref()
            .ok_or_else(|| JobsError::Unavailable("queue is disabled".into()))
    }
}

impl IntoResponse for JobsError {
    fn into_response(self) -> Response {
        let (status, code) = match self {
            Self::Forbidden(_) => (StatusCode::FORBIDDEN, "jobs.forbidden"),
            Self::NotFound => (StatusCode::NOT_FOUND, "jobs.not_found"),
            Self::Unavailable(_) => (StatusCode::SERVICE_UNAVAILABLE, "jobs.unavailable"),
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

impl std::fmt::Display for JobsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Forbidden(scope) => write!(formatter, "missing required scope `{scope}`"),
            Self::NotFound => formatter.write_str("job not found or cannot be retried"),
            Self::Unavailable(message) => write!(formatter, "job service unavailable: {message}"),
        }
    }
}
