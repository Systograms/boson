//! Operational read models shared by the Server, Worker, and Admin API.

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use axum::{Json, Router, extract::State, routing::get};
use boson_capability::{Capability, CapabilityDescriptor};
use boson_db::Database;
use boson_kernel::PlatformConfig;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::RwLock;

const TRACE_CAPACITY: usize = 500;

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

#[derive(Clone, Default)]
pub struct OpsState {
    requests: Arc<AtomicU64>,
    errors: Arc<AtomicU64>,
    traces: Arc<RwLock<VecDeque<RequestTrace>>>,
    workers: Arc<RwLock<Vec<WorkerStatus>>>,
}

impl OpsState {
    pub async fn record(&self, trace: RequestTrace) {
        self.requests.fetch_add(1, Ordering::Relaxed);
        if trace.status_code >= 500 {
            self.errors.fetch_add(1, Ordering::Relaxed);
        }
        let mut traces = self.traces.write().await;
        if traces.len() == TRACE_CAPACITY {
            traces.pop_front();
        }
        traces.push_back(trace);
    }

    pub async fn traces(&self) -> Vec<RequestTrace> {
        self.traces.read().await.iter().rev().cloned().collect()
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

async fn admin_config(State(state): State<OpsCapabilityState>) -> Json<Value> {
    Json(json!({
        "snapshot_id": state.config.snapshot_id(),
        "effective": state.config.redacted(),
        "read_only": true
    }))
}
