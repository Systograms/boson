//! Provider-agnostic contracts. Vendor SDK types must never appear here.

use std::{collections::BTreeMap, time::Duration};

use async_trait::async_trait;
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PortError {
    #[error("resource not found")]
    NotFound,
    #[error("provider unavailable: {0}")]
    Unavailable(String),
    #[error("invalid provider request: {0}")]
    Invalid(String),
    #[error("provider error: {0}")]
    Provider(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthStatus {
    pub component: String,
    pub healthy: bool,
    pub message: Option<String>,
    pub latency_ms: u64,
}

#[async_trait]
pub trait HealthCheck: Send + Sync {
    async fn check(&self) -> HealthStatus;
}

#[derive(Debug, Clone)]
pub struct ObjectMetadata {
    pub content_type: Option<String>,
    pub custom: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct Object {
    pub bytes: Bytes,
    pub metadata: ObjectMetadata,
}

#[async_trait]
pub trait ObjectStore: HealthCheck + Send + Sync {
    async fn put(&self, key: &str, bytes: Bytes, metadata: ObjectMetadata)
    -> Result<(), PortError>;
    async fn get(&self, key: &str) -> Result<Object, PortError>;
    async fn delete(&self, key: &str) -> Result<(), PortError>;
    async fn signed_upload_url(&self, key: &str, ttl: Duration) -> Result<String, PortError>;
    async fn signed_download_url(&self, key: &str, ttl: Duration) -> Result<String, PortError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Email {
    pub to: String,
    pub from: String,
    pub subject: String,
    pub text: String,
    pub idempotency_key: String,
}

#[async_trait]
pub trait Mailer: HealthCheck + Send + Sync {
    async fn send(&self, email: Email) -> Result<(), PortError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobEnvelope {
    pub id: String,
    pub topic: String,
    pub payload: serde_json::Value,
    pub attempts: u32,
    pub correlation_id: Option<String>,
}

#[async_trait]
pub trait Queue: HealthCheck + Send + Sync {
    async fn enqueue(&self, job: JobEnvelope) -> Result<(), PortError>;
    async fn lease(
        &self,
        limit: usize,
        visibility: Duration,
    ) -> Result<Vec<JobEnvelope>, PortError>;
    async fn acknowledge(&self, id: &str) -> Result<(), PortError>;
    async fn retry(&self, id: &str) -> Result<(), PortError>;
    async fn list(&self, limit: usize) -> Result<Vec<JobEnvelope>, PortError>;
}
