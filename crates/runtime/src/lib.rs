//! Shared composition runtime for Boson server and worker hosts.

use std::{
    env,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use boson_admin::{AdminAuth, AdminCapability};
use boson_audit::AuditCapability;
use boson_capability::MigrationSet;
pub use boson_capability::{Capability, CapabilityRegistry};
use boson_database_inspection::DatabaseInspectionCapability;
use boson_db::{Database, PostgresInspector};
use boson_event_log::EventsCapability;
use boson_events::EventConsumer;
use boson_files::FilesCapability;
use boson_identity::{IdentityAuth, IdentityCapability, IdentityDirectory};
use boson_jobs::JobsCapability;
use boson_kernel::{
    MailConfig, PlatformConfig, QueueConfig, RequestContext, StorageConfig, init_telemetry,
};
use boson_mailer_local::LocalMailer;
use boson_mailer_smtp::SmtpMailer;
use boson_notifications::NotificationsCapability;
use boson_ops::{OpsCapability, OpsState, RequestTrace};
use boson_organizations::OrganizationsCapability;
use boson_ports::{DatabaseInspector, Mailer, ObjectStore, Queue};
use boson_queue_postgres::PostgresQueue;
use boson_storage_local::LocalObjectStore;
use boson_storage_s3::S3ObjectStore;
use serde_json::{Value, json};
use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

/// Services available to application extension callbacks.
#[derive(Clone)]
pub struct RuntimeContext {
    pub config: Arc<PlatformConfig>,
    pub database: Option<Database>,
    pub identity_auth: IdentityAuth,
    pub identity_directory: IdentityDirectory,
    pub object_store: Arc<dyn ObjectStore>,
    pub mailer: Arc<dyn Mailer>,
    pub queue: Option<Arc<dyn Queue>>,
}

type ExtensionCallback =
    Box<dyn FnOnce(&RuntimeContext, &mut CapabilityRegistry) -> Result<()> + Send>;

/// Builds and runs a Boson host with optional application capabilities.
pub struct Builder {
    config_path: PathBuf,
    /// Optional override for core migrations. When unset, embedded platform
    /// migrations compiled into this crate are used.
    core_migrations: Option<PathBuf>,
    extensions: Vec<ExtensionCallback>,
}

impl Default for Builder {
    fn default() -> Self {
        Self {
            config_path: env::var("BOSON_CONFIG")
                .map_or_else(|_| PathBuf::from(".boson/config.yaml"), PathBuf::from),
            core_migrations: None,
            extensions: Vec::new(),
        }
    }
}

impl Builder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn from_env() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn config_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.config_path = path.into();
        self
    }

    #[must_use]
    pub fn core_migrations(mut self, path: impl Into<PathBuf>) -> Self {
        self.core_migrations = Some(path.into());
        self
    }

    /// Appends an application installer. Multiple calls accumulate in order.
    #[must_use]
    pub fn register<F>(mut self, installer: F) -> Self
    where
        F: FnOnce(&RuntimeContext, &mut CapabilityRegistry) -> Result<()> + Send + 'static,
    {
        self.extensions.push(Box::new(installer));
        self
    }

    /// Builds and registers one capability.
    #[must_use]
    pub fn capability<F>(self, build: F) -> Self
    where
        F: FnOnce(&RuntimeContext) -> Result<Arc<dyn Capability>> + Send + 'static,
    {
        self.register(move |ctx, registry| {
            registry.register(build(ctx)?)?;
            Ok(())
        })
    }

    /// Appends an application installer.
    ///
    /// Prefer [`Builder::register`]. This alias is retained for compatibility
    /// and accumulates rather than overwriting earlier installers.
    #[must_use]
    pub fn extend<F>(self, extension: F) -> Self
    where
        F: FnOnce(&RuntimeContext, &mut CapabilityRegistry) -> Result<()> + Send + 'static,
    {
        self.register(extension)
    }

    /// Applies core and capability-owned migrations.
    ///
    /// # Errors
    ///
    /// Returns an error when configuration, database connectivity, capability
    /// registration, or migration application fails.
    pub async fn migrate(self) -> Result<()> {
        let _prepared = self.prepare(true).await?;
        Ok(())
    }

    /// Starts the HTTP server after composing platform and application capabilities.
    ///
    /// # Errors
    ///
    /// Returns an error when startup composition or the HTTP listener fails.
    pub async fn run_server(self) -> Result<()> {
        let prepared = self.prepare(false).await?;
        prepared.run_server().await
    }

    /// Starts the background worker after composing platform and application capabilities.
    ///
    /// # Errors
    ///
    /// Returns an error when startup composition or worker dispatch fails.
    pub async fn run_worker(self) -> Result<()> {
        let prepared = self.prepare(false).await?;
        prepared.run_worker().await
    }

    async fn prepare(mut self, force_migrate: bool) -> Result<PreparedRuntime> {
        let config = Arc::new(PlatformConfig::load(&self.config_path)?);
        init_telemetry(&config.telemetry)?;

        let database = if config.database.connect_on_boot {
            Some(
                Database::connect(&config.database)
                    .await
                    .context("connect to PostgreSQL")?,
            )
        } else {
            None
        };

        let object_store = build_object_store(&config.storage).await?;
        let mailer = build_mailer(&config.mail).await?;
        let queue = build_queue(&config.queue, database.as_ref())?;
        let ops = OpsState::new(database.clone());
        let admin_auth = AdminAuth::new(database.clone(), &config.admin);
        let admin = AdminCapability::new(database.clone());
        let identity = IdentityCapability::new(database.clone(), &config.auth);
        let identity_auth = identity.auth();
        let identity_directory = identity.directory();

        let mut capabilities = CapabilityRegistry::default();
        capabilities.register(Arc::new(OpsCapability::new(
            Arc::clone(&config),
            database.clone(),
            ops.clone(),
        )))?;
        capabilities.register(Arc::new(admin.clone()))?;
        capabilities.register(Arc::new(NotificationsCapability::new(
            database.clone(),
            Arc::clone(&mailer),
            config.mail.from.clone(),
            config.mail.public_app_url.clone(),
        )))?;
        capabilities.register(Arc::new(AuditCapability::new(database.clone())))?;
        let database_inspector = if config.database_inspection.enabled {
            database.as_ref().map(|database| {
                Arc::new(PostgresInspector::new(
                    database.pool().clone(),
                    &config.database_inspection,
                )) as Arc<dyn DatabaseInspector>
            })
        } else {
            None
        };
        capabilities.register(Arc::new(DatabaseInspectionCapability::new(
            database_inspector,
        )))?;
        capabilities.register(Arc::new(identity))?;
        capabilities.register(Arc::new(OrganizationsCapability::new(
            database.clone(),
            identity_auth.clone(),
            identity_directory.clone(),
        )))?;
        capabilities.register(Arc::new(FilesCapability::new(
            database.clone(),
            identity_auth.clone(),
            Arc::clone(&object_store),
        )))?;
        capabilities.register(Arc::new(JobsCapability::new(queue.clone())))?;
        capabilities.register(Arc::new(EventsCapability::new(database.clone())))?;

        let context = RuntimeContext {
            config: Arc::clone(&config),
            database: database.clone(),
            identity_auth,
            identity_directory,
            object_store,
            mailer,
            queue: queue.clone(),
        };

        for extension in self.extensions.drain(..) {
            extension(&context, &mut capabilities)?;
        }

        admin.set_issued_scopes(capabilities.scopes());
        ops.set_health_checks(capabilities.health_checks()).await;

        let should_migrate =
            force_migrate || (config.database.connect_on_boot && config.database.run_migrations);
        if should_migrate {
            let Some(database) = &database else {
                bail!("migrations require database.connect_on_boot=true");
            };
            run_migrations(
                database,
                self.core_migrations.as_deref(),
                &capabilities.migrations(),
            )
            .await?;
        }

        Ok(PreparedRuntime {
            config,
            database,
            admin_auth,
            ops,
            capabilities,
            queue,
        })
    }
}

struct PreparedRuntime {
    config: Arc<PlatformConfig>,
    database: Option<Database>,
    admin_auth: AdminAuth,
    ops: OpsState,
    capabilities: CapabilityRegistry,
    queue: Option<Arc<dyn Queue>>,
}

impl PreparedRuntime {
    async fn run_server(self) -> Result<()> {
        let address = format!("{}:{}", self.config.http.host, self.config.http.port);
        let state = AppState {
            database: self.database,
            admin_auth: self.admin_auth,
            ops: self.ops,
        };
        let dashboard_dir = (!self.config.http.dashboard_dir.trim().is_empty())
            .then(|| PathBuf::from(&self.config.http.dashboard_dir));
        let app = build_router(state, &self.capabilities, dashboard_dir.as_deref());
        let listener = tokio::net::TcpListener::bind(&address).await?;
        tracing::info!(%address, "Boson server listening");
        if let Some(dir) = &dashboard_dir {
            tracing::info!(dashboard_dir = %dir.display(), "serving Dashboard assets");
        }
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await?;
        Ok(())
    }

    async fn run_worker(self) -> Result<()> {
        if !self.config.database.connect_on_boot {
            bail!("worker requires database.connect_on_boot=true");
        }
        let database = self
            .database
            .clone()
            .context("worker requires a PostgreSQL connection")?;
        let queue = self.queue.context("worker requires a queue provider")?;
        let job_handlers = self.capabilities.job_handlers();
        let event_consumers = self.capabilities.event_consumers();
        let worker_id = format!("worker-{}", std::process::id());
        tracing::info!(
            capabilities = self.capabilities.descriptors().len(),
            consumers = event_consumers.len(),
            jobs = job_handlers.len(),
            schedules = self.capabilities.schedules().len(),
            "Boson worker started"
        );

        let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
        let mut dispatch = tokio::time::interval(Duration::from_millis(
            self.config.queue.poll_interval_ms.max(1),
        ));
        loop {
            tokio::select! {
                _ = heartbeat.tick() => {
                    if let Err(error) = database.heartbeat("default").await {
                        tracing::error!(%error, "failed to record worker heartbeat");
                    }
                }
                _ = dispatch.tick() => {
                    if let Err(error) = dispatch_jobs(
                        queue.as_ref(),
                        &job_handlers,
                        &self.config.queue,
                        &worker_id,
                    ).await {
                        tracing::error!(%error, "job dispatch cycle failed");
                    }
                    if let Err(error) = dispatch_events(
                        &database,
                        &event_consumers,
                        &self.config.queue,
                        &worker_id,
                    ).await {
                        tracing::error!(%error, "event dispatch cycle failed");
                    }
                }
                () = shutdown_signal() => {
                    tracing::info!("worker shutdown signal received");
                    break;
                }
            }
        }
        Ok(())
    }
}

static CORE_MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("../../migrations");

/// Applies platform and capability migrations in dependency order.
///
/// Platform migrations are embedded in the runtime binary. `core_migrations`
/// remains available as an explicit override for development and testing.
///
/// Capability migrations are tracked in `_sqlx_migrations_<owner>` tables so
/// they never collide with the platform `_sqlx_migrations` history.
///
/// # Errors
///
/// Returns an error when a migration path is missing or SQL migration fails.
pub async fn run_migrations(
    database: &Database,
    core_migrations: Option<&Path>,
    capability_migrations: &[MigrationSet],
) -> Result<()> {
    if let Some(core) = core_migrations {
        if !core.exists() {
            bail!("core migrations path {} does not exist", core.display());
        }
        database
            .migrate(core)
            .await
            .with_context(|| format!("run core migrations from {}", core.display()))?;
    } else {
        database
            .migrate_embedded(&CORE_MIGRATOR)
            .await
            .context("run embedded core migrations")?;
    }
    for migration in capability_migrations {
        if !migration.path.exists() {
            bail!(
                "capability `{}` migration path {} does not exist",
                migration.owner,
                migration.path.display()
            );
        }
        let table = format!("_sqlx_migrations_{}", migration.owner.replace('-', "_"));
        database
            .migrate_with_table(&migration.path, &table)
            .await
            .with_context(|| {
                format!(
                    "run migrations for capability `{}` from {}",
                    migration.owner,
                    migration.path.display()
                )
            })?;
    }
    Ok(())
}

#[derive(Clone)]
struct AppState {
    database: Option<Database>,
    admin_auth: AdminAuth,
    ops: OpsState,
}

fn build_router(
    state: AppState,
    capabilities: &CapabilityRegistry,
    dashboard_dir: Option<&Path>,
) -> Router {
    let admin = capabilities
        .admin_router()
        .route_layer(middleware::from_fn_with_state(state.clone(), require_admin));
    let mut core = Router::new()
        .route("/healthz", get(liveness))
        .route("/readyz", get(readiness));
    if dashboard_dir.is_none() {
        core = core.route("/", get(root));
    }
    let core = core.with_state(state.clone());

    let mut router = Router::new()
        .merge(core)
        .nest("/v1", capabilities.app_router())
        .nest("/admin/v1", admin)
        .layer(middleware::from_fn_with_state(state, trace_request))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive());

    if let Some(dir) = dashboard_dir {
        let index = dir.join("index.html");
        let serve = ServeDir::new(dir)
            .append_index_html_on_directories(true)
            .not_found_service(ServeFile::new(index));
        router = router.fallback_service(serve);
    }

    router
}

async fn root() -> Json<Value> {
    Json(json!({
        "name": "Boson",
        "description": "A modular backend platform",
        "version": env!("CARGO_PKG_VERSION"),
        "admin_api": "/admin/v1"
    }))
}

async fn liveness() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn readiness(State(state): State<AppState>) -> impl IntoResponse {
    match &state.database {
        Some(database) => match database.ping().await {
            Ok(()) => (StatusCode::OK, Json(json!({ "status": "ready" }))),
            Err(error) => (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "status": "not_ready", "error": error.to_string() })),
            ),
        },
        None => (
            StatusCode::OK,
            Json(json!({ "status": "ready", "database": "disabled" })),
        ),
    }
}

async fn require_admin(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let supplied = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));

    let Some(token) = supplied else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": {
                    "code": "admin.unauthorized",
                    "message": "A valid admin bearer token is required"
                }
            })),
        )
            .into_response();
    };
    match state.admin_auth.authenticate(token).await {
        Ok(principal) => {
            let mut request = request;
            request.extensions_mut().insert(principal);
            next.run(request).await
        }
        Err(error) => error.into_response(),
    }
}

async fn trace_request(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let context = RequestContext::new();
    let request_id = context.request_id.to_string();
    let started_at = context.started_at;
    let method = request.method().to_string();
    let path = request.uri().path().to_owned();
    request.extensions_mut().insert(context);

    let started = Instant::now();
    let mut response = next.run(request).await;
    let duration_ms = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
    let status_code = response.status().as_u16();
    if let Ok(value) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert("x-request-id", value);
    }

    state
        .ops
        .record(RequestTrace {
            request_id,
            started_at,
            method,
            path,
            status_code,
            duration_ms,
        })
        .await;
    response
}

async fn build_object_store(storage: &StorageConfig) -> Result<Arc<dyn ObjectStore>> {
    match storage.provider.as_str() {
        "local" => {
            let store = LocalObjectStore::open(&storage.local_root)
                .await
                .context("open local object store root")?;
            Ok(Arc::new(store))
        }
        "s3" => {
            let store = S3ObjectStore::from_config(storage)
                .context("configure S3-compatible object store")?;
            Ok(Arc::new(store))
        }
        other => bail!("unsupported storage.provider `{other}`; supported providers: local, s3"),
    }
}

async fn build_mailer(config: &MailConfig) -> Result<Arc<dyn Mailer>> {
    match config.provider.as_str() {
        "local" => Ok(Arc::new(
            LocalMailer::open(&config.local_root)
                .await
                .context("open local mailbox")?,
        )),
        "smtp" => Ok(Arc::new(
            SmtpMailer::from_config(config).context("configure SMTP mailer")?,
        )),
        other => bail!("unsupported mail.provider `{other}`; supported providers: local, smtp"),
    }
}

fn build_queue(
    config: &QueueConfig,
    database: Option<&Database>,
) -> Result<Option<Arc<dyn Queue>>> {
    match config.provider.as_str() {
        "postgres" => Ok(database.map(|database| {
            Arc::new(PostgresQueue::new(
                database.pool().clone(),
                config.max_attempts,
            )) as Arc<dyn Queue>
        })),
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
            .filter(|consumer| consumer.topic() == "*" || consumer.topic() == event.envelope.topic)
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

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("install Ctrl+C signal handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install terminate signal handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutdown signal received");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_accumulates_installers() {
        let mut builder = Builder::new();
        assert!(builder.extensions.is_empty());
        builder = builder
            .register(|_ctx, _registry| Ok(()))
            .extend(|_ctx, _registry| Ok(()))
            .capability(|_ctx| bail!("capability builders are only invoked during prepare"));
        assert_eq!(builder.extensions.len(), 3);
    }

    #[test]
    fn core_migrations_are_embedded_in_release_order() {
        let versions = CORE_MIGRATOR
            .iter()
            .map(|migration| migration.version)
            .collect::<Vec<_>>();

        assert_eq!(versions.len(), 9);
        assert_eq!(versions.first(), Some(&20_260_718_110_000));
        assert_eq!(versions.last(), Some(&20_260_718_190_000));
    }

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

    #[test]
    fn unknown_queue_provider_fails_closed() {
        let config = QueueConfig {
            provider: "memory".into(),
            ..QueueConfig::default()
        };
        assert!(build_queue(&config, None).is_err());
    }

    #[tokio::test]
    async fn unknown_storage_provider_fails_closed() {
        let config = StorageConfig {
            provider: "gcs".into(),
            ..StorageConfig::default()
        };
        assert!(build_object_store(&config).await.is_err());
    }

    #[tokio::test]
    async fn misconfigured_s3_storage_fails_closed() {
        let config = StorageConfig {
            provider: "s3".into(),
            ..StorageConfig::default()
        };
        assert!(build_object_store(&config).await.is_err());
    }

    #[tokio::test]
    async fn unknown_mail_provider_fails_closed() {
        let config = MailConfig {
            provider: "sendgrid".into(),
            ..MailConfig::default()
        };
        assert!(build_mailer(&config).await.is_err());
    }

    #[tokio::test]
    async fn smtp_mailer_without_host_fails_closed() {
        let config = MailConfig {
            provider: "smtp".into(),
            ..MailConfig::default()
        };
        assert!(build_mailer(&config).await.is_err());
    }
}
