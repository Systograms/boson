//! Operational read models shared by the Server, Worker, and Admin API.

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use chrono::{DateTime, Utc};
use serde::Serialize;
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
