//! Provider-agnostic contracts. Vendor SDK types must never appear here.

use std::{collections::BTreeMap, time::Duration};

use async_trait::async_trait;
use bytes::Bytes;
use chrono::{DateTime, Utc};
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
    pub max_attempts: u32,
    pub correlation_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Dead,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobRecord {
    pub envelope: JobEnvelope,
    pub status: JobStatus,
    pub run_at: DateTime<Utc>,
    pub locked_at: Option<DateTime<Utc>>,
    pub locked_by: Option<String>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[async_trait]
pub trait Queue: HealthCheck + Send + Sync {
    async fn enqueue(&self, job: JobEnvelope) -> Result<(), PortError>;
    async fn lease(
        &self,
        limit: usize,
        visibility: Duration,
        worker_id: &str,
    ) -> Result<Vec<JobEnvelope>, PortError>;
    async fn acknowledge(&self, id: &str, worker_id: &str) -> Result<(), PortError>;
    /// Releases a leased job. The adapter increments attempts and marks the job
    /// dead once `max_attempts` is reached.
    async fn retry(
        &self,
        id: &str,
        worker_id: &str,
        error: Option<&str>,
        delay: Duration,
    ) -> Result<JobStatus, PortError>;
    /// Manually requeues a failed or dead job without erasing its history.
    async fn requeue(&self, id: &str) -> Result<(), PortError>;
    async fn list(&self, limit: usize) -> Result<Vec<JobRecord>, PortError>;
}

/// Provider-neutral identity for a database table. `namespace` maps to a
/// PostgreSQL schema, a MySQL database, or the closest equivalent offered by
/// another adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TableRef {
    pub namespace: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RowCount {
    pub value: u64,
    pub exact: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableSummary {
    pub table: TableRef,
    pub primary_key: Vec<String>,
    pub row_count: Option<RowCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnSchema {
    pub name: String,
    /// Display-oriented provider type. Consumers must not parse this to make
    /// behavioral decisions.
    pub data_type: String,
    pub nullable: bool,
    pub primary_key: bool,
    pub redacted: bool,
    pub default: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForeignKeySchema {
    pub name: String,
    pub columns: Vec<String>,
    pub referenced_table: TableRef,
    pub referenced_columns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableSchema {
    pub table: TableRef,
    pub columns: Vec<ColumnSchema>,
    pub primary_key: Vec<String>,
    pub foreign_keys: Vec<ForeignKeySchema>,
    pub row_count: Option<RowCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CellKind {
    Null,
    Boolean,
    Number,
    Text,
    Json,
    Binary,
    DateTime,
    Other,
    Redacted,
}

/// Values are transported as strings to preserve 64-bit integers, decimals,
/// timestamps, and provider-specific values without JavaScript precision loss.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellValue {
    pub kind: CellKind,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseRow {
    pub cells: BTreeMap<String, CellValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnFilter {
    pub column: String,
    pub value: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RowQuery {
    pub limit: u32,
    /// Opaque adapter-owned pagination token.
    pub cursor: Option<String>,
    /// Phase-one filters are exact matches. Adapters must bind values rather
    /// than interpolate them into provider queries.
    pub filters: Vec<ColumnFilter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RowPage {
    pub table: TableRef,
    pub columns: Vec<ColumnSchema>,
    pub rows: Vec<DatabaseRow>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseInspectorCapabilities {
    pub provider: String,
    pub supports_namespaces: bool,
    pub supports_exact_count: bool,
    pub max_page_size: u32,
}

/// Privileged, read-only metadata and row inspection. Implementations must
/// validate identifiers against provider metadata, bind filter values, enforce
/// page limits, and execute row reads in read-only transactions.
#[async_trait]
pub trait DatabaseInspector: Send + Sync {
    fn capabilities(&self) -> DatabaseInspectorCapabilities;

    async fn list_tables(&self) -> Result<Vec<TableSummary>, PortError>;

    async fn describe_table(&self, table: &TableRef) -> Result<TableSchema, PortError>;

    async fn query_rows(&self, table: &TableRef, query: RowQuery) -> Result<RowPage, PortError>;
}
