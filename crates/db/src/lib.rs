//! PostgreSQL foundation: connection pool, migrations, and transactional outbox.

use std::{path::Path, time::Duration};

use boson_events::EventEnvelope;
use boson_kernel::DatabaseConfig;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
}

#[derive(Clone)]
pub struct Database {
    pool: PgPool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkerHeartbeat {
    pub name: String,
    pub last_heartbeat: DateTime<Utc>,
}

impl Database {
    pub async fn connect(config: &DatabaseConfig) -> Result<Self, DatabaseError> {
        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .acquire_timeout(Duration::from_secs(5))
            .connect(&config.url)
            .await?;
        Ok(Self { pool })
    }

    pub async fn migrate(&self, path: impl AsRef<Path>) -> Result<(), DatabaseError> {
        sqlx::migrate::Migrator::new(path.as_ref())
            .await?
            .run(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn ping(&self) -> Result<(), DatabaseError> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    pub async fn publish(&self, event: &EventEnvelope) -> Result<(), DatabaseError> {
        sqlx::query(
            "INSERT INTO kernel.outbox
             (id, topic, payload, correlation_id, occurred_at)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(event.id)
        .bind(&event.topic)
        .bind(&event.payload)
        .bind(&event.correlation_id)
        .bind(event.occurred_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn heartbeat(&self, worker_name: &str) -> Result<(), DatabaseError> {
        sqlx::query(
            "INSERT INTO ops.worker_heartbeats (name, last_heartbeat)
             VALUES ($1, now())
             ON CONFLICT (name)
             DO UPDATE SET last_heartbeat = excluded.last_heartbeat",
        )
        .bind(worker_name)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn worker_heartbeats(&self) -> Result<Vec<WorkerHeartbeat>, DatabaseError> {
        let rows = sqlx::query(
            "SELECT name, last_heartbeat
             FROM ops.worker_heartbeats
             ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(WorkerHeartbeat {
                    name: row.try_get("name")?,
                    last_heartbeat: row.try_get("last_heartbeat")?,
                })
            })
            .collect()
    }

    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}
