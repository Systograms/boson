//! `PostgreSQL` implementation of the provider-neutral queue port.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use boson_ports::{HealthCheck, HealthStatus, JobEnvelope, JobRecord, JobStatus, PortError, Queue};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};

#[derive(Clone)]
pub struct PostgresQueue {
    pool: PgPool,
    default_max_attempts: u32,
}

impl PostgresQueue {
    #[must_use]
    pub fn new(pool: PgPool, default_max_attempts: u32) -> Self {
        Self {
            pool,
            default_max_attempts: default_max_attempts.max(1),
        }
    }
}

#[async_trait]
impl HealthCheck for PostgresQueue {
    async fn check(&self) -> HealthStatus {
        let started = Instant::now();
        let result = sqlx::query("SELECT 1").execute(&self.pool).await;
        HealthStatus {
            component: "queue-postgres".into(),
            healthy: result.is_ok(),
            message: result.err().map(|error| error.to_string()),
            latency_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
        }
    }
}

#[async_trait]
impl Queue for PostgresQueue {
    async fn enqueue(&self, job: JobEnvelope) -> Result<(), PortError> {
        let max_attempts = if job.max_attempts == 0 {
            self.default_max_attempts
        } else {
            job.max_attempts
        };
        sqlx::query(
            "INSERT INTO kernel.jobs
             (id, topic, payload, attempts, max_attempts, correlation_id)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(job.id)
        .bind(job.topic)
        .bind(job.payload)
        .bind(i32::try_from(job.attempts).map_err(invalid_number)?)
        .bind(i32::try_from(max_attempts).map_err(invalid_number)?)
        .bind(job.correlation_id)
        .execute(&self.pool)
        .await
        .map_err(provider)?;
        Ok(())
    }

    async fn lease(
        &self,
        limit: usize,
        visibility: Duration,
        worker_id: &str,
    ) -> Result<Vec<JobEnvelope>, PortError> {
        let limit = i64::try_from(limit).map_err(invalid_number)?;
        let visibility = i64::try_from(visibility.as_secs()).map_err(invalid_number)?;
        let mut transaction = self.pool.begin().await.map_err(provider)?;
        let rows = sqlx::query(
            "WITH candidates AS (
                SELECT id FROM kernel.jobs
                WHERE run_at <= now()
                  AND (
                    status IN ('pending', 'failed')
                    OR (status = 'running'
                        AND locked_at < now() - make_interval(secs => $2))
                  )
                ORDER BY run_at, created_at
                FOR UPDATE SKIP LOCKED
                LIMIT $1
             )
             UPDATE kernel.jobs AS jobs
             SET status = 'running', locked_at = now(), locked_by = $3,
                 updated_at = now()
             FROM candidates
             WHERE jobs.id = candidates.id
             RETURNING jobs.id, jobs.topic, jobs.payload, jobs.attempts,
                       jobs.max_attempts, jobs.correlation_id",
        )
        .bind(limit)
        .bind(visibility)
        .bind(worker_id)
        .fetch_all(&mut *transaction)
        .await
        .map_err(provider)?;
        transaction.commit().await.map_err(provider)?;
        rows.iter().map(envelope_from_row).collect()
    }

    async fn acknowledge(&self, id: &str, worker_id: &str) -> Result<(), PortError> {
        let result = sqlx::query(
            "UPDATE kernel.jobs
             SET status = 'completed', locked_at = NULL, locked_by = NULL,
                 last_error = NULL, updated_at = now()
             WHERE id = $1 AND status = 'running' AND locked_by = $2",
        )
        .bind(id)
        .bind(worker_id)
        .execute(&self.pool)
        .await
        .map_err(provider)?;
        if result.rows_affected() == 0 {
            return Err(PortError::NotFound);
        }
        Ok(())
    }

    async fn retry(
        &self,
        id: &str,
        worker_id: &str,
        error: Option<&str>,
        delay: Duration,
    ) -> Result<JobStatus, PortError> {
        let delay = i64::try_from(delay.as_secs()).map_err(invalid_number)?;
        let row = sqlx::query(
            "UPDATE kernel.jobs
             SET attempts = attempts + 1,
                 status = CASE WHEN attempts + 1 >= max_attempts
                               THEN 'dead' ELSE 'failed' END,
                 run_at = CASE WHEN attempts + 1 >= max_attempts
                               THEN run_at
                               ELSE now() + make_interval(secs => $4) END,
                 locked_at = NULL, locked_by = NULL, last_error = $3,
                 updated_at = now()
             WHERE id = $1 AND status = 'running' AND locked_by = $2
             RETURNING status",
        )
        .bind(id)
        .bind(worker_id)
        .bind(error)
        .bind(delay)
        .fetch_optional(&self.pool)
        .await
        .map_err(provider)?
        .ok_or(PortError::NotFound)?;
        status_from_str(row.try_get("status").map_err(provider)?)
    }

    async fn requeue(&self, id: &str) -> Result<(), PortError> {
        let additional = i32::try_from(self.default_max_attempts).map_err(invalid_number)?;
        let result = sqlx::query(
            "UPDATE kernel.jobs
             SET status = 'pending', run_at = now(), locked_at = NULL,
                 locked_by = NULL,
                 max_attempts = GREATEST(max_attempts, attempts + $2),
                 updated_at = now()
             WHERE id = $1 AND status IN ('failed', 'dead')",
        )
        .bind(id)
        .bind(additional)
        .execute(&self.pool)
        .await
        .map_err(provider)?;
        if result.rows_affected() == 0 {
            return Err(PortError::NotFound);
        }
        Ok(())
    }

    async fn list(&self, limit: usize) -> Result<Vec<JobRecord>, PortError> {
        let rows = sqlx::query(
            "SELECT id, topic, payload, status, attempts, max_attempts,
                    run_at, locked_at, locked_by, last_error, correlation_id,
                    created_at, updated_at
             FROM kernel.jobs ORDER BY created_at DESC LIMIT $1",
        )
        .bind(i64::try_from(limit).map_err(invalid_number)?)
        .fetch_all(&self.pool)
        .await
        .map_err(provider)?;
        rows.iter().map(record_from_row).collect()
    }
}

fn envelope_from_row(row: &sqlx::postgres::PgRow) -> Result<JobEnvelope, PortError> {
    Ok(JobEnvelope {
        id: row.try_get("id").map_err(provider)?,
        topic: row.try_get("topic").map_err(provider)?,
        payload: row.try_get("payload").map_err(provider)?,
        attempts: to_u32(row.try_get("attempts").map_err(provider)?)?,
        max_attempts: to_u32(row.try_get("max_attempts").map_err(provider)?)?,
        correlation_id: row.try_get("correlation_id").map_err(provider)?,
    })
}

fn record_from_row(row: &sqlx::postgres::PgRow) -> Result<JobRecord, PortError> {
    Ok(JobRecord {
        envelope: envelope_from_row(row)?,
        status: status_from_str(row.try_get("status").map_err(provider)?)?,
        run_at: row
            .try_get::<DateTime<Utc>, _>("run_at")
            .map_err(provider)?,
        locked_at: row.try_get("locked_at").map_err(provider)?,
        locked_by: row.try_get("locked_by").map_err(provider)?,
        last_error: row.try_get("last_error").map_err(provider)?,
        created_at: row.try_get("created_at").map_err(provider)?,
        updated_at: row.try_get("updated_at").map_err(provider)?,
    })
}

fn status_from_str(status: &str) -> Result<JobStatus, PortError> {
    match status {
        "pending" => Ok(JobStatus::Pending),
        "running" => Ok(JobStatus::Running),
        "completed" => Ok(JobStatus::Completed),
        "failed" => Ok(JobStatus::Failed),
        "dead" => Ok(JobStatus::Dead),
        other => Err(PortError::Provider(format!(
            "database returned unknown job status `{other}`"
        ))),
    }
}

fn to_u32(value: i32) -> Result<u32, PortError> {
    u32::try_from(value).map_err(invalid_number)
}

#[allow(clippy::needless_pass_by_value)]
fn provider(error: impl ToString) -> PortError {
    PortError::Provider(error.to_string())
}

#[allow(clippy::needless_pass_by_value)]
fn invalid_number(error: impl ToString) -> PortError {
    PortError::Invalid(error.to_string())
}
