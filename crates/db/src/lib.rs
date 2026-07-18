//! PostgreSQL foundation: connection pool, migrations, and transactional outbox.

use std::{path::Path, time::Duration};

use boson_events::EventEnvelope;
use boson_kernel::DatabaseConfig;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{PgPool, Row, postgres::PgPoolOptions};
use thiserror::Error;
use uuid::Uuid;

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

#[derive(Debug, Clone)]
pub struct LeasedEvent {
    pub envelope: EventEnvelope,
    pub attempts: u32,
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

    pub async fn lease_events(
        &self,
        limit: usize,
        visibility: Duration,
        worker_id: &str,
    ) -> Result<Vec<LeasedEvent>, DatabaseError> {
        let mut transaction = self.pool.begin().await?;
        let rows = sqlx::query(
            "WITH candidates AS (
                SELECT id FROM kernel.outbox
                WHERE dispatched_at IS NULL AND run_at <= now()
                  AND (
                    status = 'pending'
                    OR (status = 'processing'
                        AND locked_at < now() - make_interval(secs => $2))
                  )
                ORDER BY run_at, created_at
                FOR UPDATE SKIP LOCKED
                LIMIT $1
             )
             UPDATE kernel.outbox AS events
             SET status = 'processing', locked_at = now(), locked_by = $3
             FROM candidates
             WHERE events.id = candidates.id
             RETURNING events.id, events.topic, events.payload,
                       events.correlation_id, events.occurred_at, events.attempts",
        )
        .bind(i64::try_from(limit).unwrap_or(i64::MAX))
        .bind(i64::try_from(visibility.as_secs()).unwrap_or(i64::MAX))
        .bind(worker_id)
        .fetch_all(&mut *transaction)
        .await?;
        transaction.commit().await?;
        rows.into_iter()
            .map(|row| {
                Ok(LeasedEvent {
                    envelope: EventEnvelope {
                        id: row.try_get("id")?,
                        topic: row.try_get("topic")?,
                        occurred_at: row.try_get("occurred_at")?,
                        correlation_id: row.try_get("correlation_id")?,
                        actor_id: None,
                        payload: row.try_get("payload")?,
                    },
                    attempts: u32::try_from(row.try_get::<i32, _>("attempts")?).unwrap_or(0),
                })
            })
            .collect()
    }

    pub async fn delivered_consumers(&self, event_id: Uuid) -> Result<Vec<String>, DatabaseError> {
        let rows = sqlx::query(
            "SELECT consumer FROM kernel.event_deliveries
             WHERE event_id = $1 AND status = 'succeeded'",
        )
        .bind(event_id)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| row.try_get("consumer").map_err(DatabaseError::from))
            .collect()
    }

    pub async fn record_delivery(
        &self,
        event_id: Uuid,
        consumer: &str,
        error: Option<&str>,
    ) -> Result<(), DatabaseError> {
        sqlx::query(
            "INSERT INTO kernel.event_deliveries
             (event_id, consumer, status, attempts, last_error,
              first_attempted_at, last_attempted_at, delivered_at)
             VALUES ($1, $2, CASE WHEN $3::TEXT IS NULL THEN 'succeeded' ELSE 'failed' END,
                     1, $3, now(), now(),
                     CASE WHEN $3::TEXT IS NULL THEN now() ELSE NULL END)
             ON CONFLICT (event_id, consumer) DO UPDATE
             SET status = excluded.status,
                 attempts = kernel.event_deliveries.attempts + 1,
                 last_error = excluded.last_error,
                 last_attempted_at = now(),
                 delivered_at = CASE WHEN excluded.status = 'succeeded'
                                     THEN now()
                                     ELSE kernel.event_deliveries.delivered_at END",
        )
        .bind(event_id)
        .bind(consumer)
        .bind(error)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn complete_event(
        &self,
        event_id: Uuid,
        worker_id: &str,
    ) -> Result<(), DatabaseError> {
        sqlx::query(
            "UPDATE kernel.outbox
             SET status = 'dispatched', dispatched_at = now(),
                 locked_at = NULL, locked_by = NULL, last_error = NULL
             WHERE id = $1 AND status = 'processing' AND locked_by = $2",
        )
        .bind(event_id)
        .bind(worker_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn retry_event(
        &self,
        event_id: Uuid,
        worker_id: &str,
        error: &str,
        delay: Duration,
    ) -> Result<(), DatabaseError> {
        sqlx::query(
            "UPDATE kernel.outbox
             SET status = 'pending', attempts = attempts + 1,
                 run_at = now() + make_interval(secs => $4),
                 locked_at = NULL, locked_by = NULL, last_error = $3
             WHERE id = $1 AND status = 'processing' AND locked_by = $2",
        )
        .bind(event_id)
        .bind(worker_id)
        .bind(error)
        .bind(i64::try_from(delay.as_secs()).unwrap_or(i64::MAX))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}
