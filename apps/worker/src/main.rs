use std::{env, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail};
use boson_admin::AdminCapability;
use boson_audit::AuditCapability;
use boson_capability::CapabilityRegistry;
use boson_db::Database;
use boson_event_log::EventsCapability;
use boson_events::EventConsumer;
use boson_files::FilesCapability;
use boson_identity::IdentityCapability;
use boson_jobs::JobsCapability;
use boson_kernel::{PlatformConfig, QueueConfig, StorageConfig, init_telemetry};
use boson_ops::{OpsCapability, OpsState};
use boson_organizations::OrganizationsCapability;
use boson_ports::{ObjectStore, Queue};
use boson_queue_postgres::PostgresQueue;
use boson_storage_local::LocalObjectStore;

#[tokio::main]
async fn main() -> Result<()> {
    let config_path = env::var("BOSON_CONFIG").unwrap_or_else(|_| "config/local.yaml".to_owned());
    let config = Arc::new(PlatformConfig::load(config_path)?);
    init_telemetry(&config.telemetry)?;

    if !config.database.connect_on_boot {
        bail!("worker requires database.connect_on_boot=true");
    }

    let database = Database::connect(&config.database)
        .await
        .context("connect worker to PostgreSQL")?;
    if config.database.run_migrations {
        database.migrate("migrations").await?;
    }

    let mut capabilities = CapabilityRegistry::default();
    capabilities.register(Arc::new(OpsCapability::new(
        Arc::clone(&config),
        Some(database.clone()),
        OpsState::new(Some(database.clone())),
    )))?;
    capabilities.register(Arc::new(AdminCapability::new(Some(database.clone()))))?;
    capabilities.register(Arc::new(AuditCapability::new(Some(database.clone()))))?;
    let identity = IdentityCapability::new(Some(database.clone()), &config.auth);
    let identity_auth = identity.auth();
    let identity_directory = identity.directory();
    capabilities.register(Arc::new(identity))?;
    capabilities.register(Arc::new(OrganizationsCapability::new(
        Some(database.clone()),
        identity_auth.clone(),
        identity_directory,
    )))?;
    let object_store = build_object_store(&config.storage).await?;
    capabilities.register(Arc::new(FilesCapability::new(
        Some(database.clone()),
        identity_auth,
        object_store,
    )))?;
    let queue = build_queue(&config.queue, &database)?;
    capabilities.register(Arc::new(JobsCapability::new(Some(Arc::clone(&queue)))))?;
    capabilities.register(Arc::new(EventsCapability::new(Some(database.clone()))))?;
    let job_handlers = capabilities.job_handlers();
    let event_consumers = capabilities.event_consumers();
    let worker_id = format!("worker-{}", std::process::id());
    tracing::info!(
        capabilities = capabilities.descriptors().len(),
        consumers = capabilities.event_consumers().len(),
        jobs = capabilities.job_handlers().len(),
        schedules = capabilities.schedules().len(),
        "Boson worker started"
    );
    let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
    let mut dispatch =
        tokio::time::interval(Duration::from_millis(config.queue.poll_interval_ms.max(1)));
    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if let Err(error) = database.heartbeat("default").await {
                    tracing::error!(%error, "failed to record worker heartbeat");
                } else {
                    tracing::debug!("worker heartbeat recorded");
                }
            }
            _ = dispatch.tick() => {
                if let Err(error) = dispatch_jobs(
                    queue.as_ref(),
                    &job_handlers,
                    &config.queue,
                    &worker_id,
                ).await {
                    tracing::error!(%error, "job dispatch cycle failed");
                }
                if let Err(error) = dispatch_events(
                    &database,
                    &event_consumers,
                    &config.queue,
                    &worker_id,
                ).await {
                    tracing::error!(%error, "event dispatch cycle failed");
                }
            }
            signal = tokio::signal::ctrl_c() => {
                signal.context("listen for worker shutdown")?;
                tracing::info!("worker shutdown signal received");
                break;
            }
        }
    }
    Ok(())
}

/// Selects the concrete object store adapter. Only `local` exists today;
/// any other provider is a startup failure, never a silent fallback.
async fn build_object_store(storage: &StorageConfig) -> Result<Arc<dyn ObjectStore>> {
    match storage.provider.as_str() {
        "local" => {
            let store = LocalObjectStore::open(&storage.local_root)
                .await
                .context("open local object store root")?;
            Ok(Arc::new(store))
        }
        other => bail!("unsupported storage.provider `{other}`; only `local` is supported"),
    }
}

fn build_queue(config: &QueueConfig, database: &Database) -> Result<Arc<dyn Queue>> {
    match config.provider.as_str() {
        "postgres" => Ok(Arc::new(PostgresQueue::new(
            database.pool().clone(),
            config.max_attempts,
        ))),
        other => bail!("unsupported queue.provider `{other}`; only `postgres` is supported"),
    }
}

async fn dispatch_jobs(
    queue: &dyn Queue,
    handlers: &[Arc<dyn boson_capability::JobHandler>],
    config: &QueueConfig,
    worker_id: &str,
) -> Result<()> {
    let jobs = queue
        .lease(
            config.batch_size,
            Duration::from_secs(config.visibility_timeout_seconds),
            worker_id,
        )
        .await
        .context("lease jobs")?;
    for job in jobs {
        let result = match handlers.iter().find(|handler| handler.name() == job.topic) {
            Some(handler) => handler
                .handle(&job)
                .await
                .map_err(|error| error.to_string()),
            None => Err(format!(
                "no job handler registered for topic `{}`",
                job.topic
            )),
        };
        match result {
            Ok(()) => queue
                .acknowledge(&job.id, worker_id)
                .await
                .with_context(|| format!("acknowledge job {}", job.id))?,
            Err(error) => {
                let status = queue
                    .retry(&job.id, worker_id, Some(&error), retry_delay(job.attempts))
                    .await
                    .with_context(|| format!("release failed job {}", job.id))?;
                tracing::warn!(job_id = %job.id, topic = %job.topic, ?status, %error, "job failed");
            }
        }
    }
    Ok(())
}

async fn dispatch_events(
    database: &Database,
    consumers: &[Arc<dyn EventConsumer>],
    config: &QueueConfig,
    worker_id: &str,
) -> Result<()> {
    let events = database
        .lease_events(
            config.batch_size,
            Duration::from_secs(config.visibility_timeout_seconds),
            worker_id,
        )
        .await
        .context("lease outbox events")?;
    for event in events {
        let matching = consumers
            .iter()
            .filter(|consumer| {
                consumer.topic() == "*" || consumer.topic() == event.envelope.topic
            })
            .collect::<Vec<_>>();
        let delivered = database
            .delivered_consumers(event.envelope.id)
            .await
            .context("load completed event deliveries")?;
        let mut failure = None;
        for consumer in matching {
            if delivered.iter().any(|name| name == consumer.name()) {
                continue;
            }
            match consumer.handle(&event.envelope).await {
                Ok(()) => database
                    .record_delivery(event.envelope.id, consumer.name(), None)
                    .await
                    .context("record successful event delivery")?,
                Err(error) => {
                    let message = error.to_string();
                    database
                        .record_delivery(event.envelope.id, consumer.name(), Some(&message))
                        .await
                        .context("record failed event delivery")?;
                    failure.get_or_insert(message);
                }
            }
        }
        match event_outcome(failure) {
            EventOutcome::Retry(error) => database
                .retry_event(
                    event.envelope.id,
                    worker_id,
                    &error,
                    retry_delay(event.attempts),
                )
                .await
                .context("release failed outbox event")?,
            EventOutcome::Complete => database
                .complete_event(event.envelope.id, worker_id)
                .await
                .context("complete outbox event")?,
        }
    }
    Ok(())
}

fn retry_delay(attempts: u32) -> Duration {
    Duration::from_secs(5 * (1_u64 << attempts.min(6)))
}

#[derive(Debug, PartialEq, Eq)]
enum EventOutcome {
    Complete,
    Retry(String),
}

fn event_outcome(failure: Option<String>) -> EventOutcome {
    failure.map_or(EventOutcome::Complete, EventOutcome::Retry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_and_job_backoff_is_bounded() {
        assert_eq!(retry_delay(0), Duration::from_secs(5));
        assert_eq!(retry_delay(10), Duration::from_secs(320));
    }

    #[test]
    fn event_without_consumers_completes() {
        assert_eq!(event_outcome(None), EventOutcome::Complete);
    }

    #[test]
    fn consumer_failure_retries_event() {
        assert_eq!(
            event_outcome(Some("consumer failed".into())),
            EventOutcome::Retry("consumer failed".into())
        );
    }
}
