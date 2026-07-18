use std::{env, sync::Arc, time::Instant};

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
use boson_capability::CapabilityRegistry;
use boson_database_inspection::DatabaseInspectionCapability;
use boson_db::{Database, PostgresInspector};
use boson_event_log::EventsCapability;
use boson_files::FilesCapability;
use boson_identity::IdentityCapability;
use boson_jobs::JobsCapability;
use boson_kernel::{PlatformConfig, QueueConfig, RequestContext, StorageConfig, init_telemetry};
use boson_ops::{OpsCapability, OpsState, RequestTrace};
use boson_organizations::OrganizationsCapability;
use boson_ports::{DatabaseInspector, ObjectStore, Queue};
use boson_queue_postgres::PostgresQueue;
use boson_storage_local::LocalObjectStore;
use serde_json::{Value, json};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

#[derive(Clone)]
struct AppState {
    database: Option<Database>,
    admin_auth: AdminAuth,
    ops: OpsState,
}

#[tokio::main]
async fn main() -> Result<()> {
    let config_path = env::var("BOSON_CONFIG").unwrap_or_else(|_| "config/local.yaml".to_owned());
    let config = Arc::new(PlatformConfig::load(config_path)?);
    init_telemetry(&config.telemetry)?;

    let database = if config.database.connect_on_boot {
        let database = Database::connect(&config.database)
            .await
            .context("connect to PostgreSQL")?;
        if config.database.run_migrations {
            database
                .migrate("migrations")
                .await
                .context("run database migrations")?;
        }
        Some(database)
    } else {
        None
    };

    let ops = OpsState::new(database.clone());
    let admin_auth = AdminAuth::new(database.clone(), &config.admin);
    let mut capabilities = CapabilityRegistry::default();
    capabilities.register(Arc::new(OpsCapability::new(
        Arc::clone(&config),
        database.clone(),
        ops.clone(),
    )))?;
    capabilities.register(Arc::new(AdminCapability::new(database.clone())))?;
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
    let identity = IdentityCapability::new(database.clone(), &config.auth);
    let identity_auth = identity.auth();
    let identity_directory = identity.directory();
    capabilities.register(Arc::new(identity))?;
    capabilities.register(Arc::new(OrganizationsCapability::new(
        database.clone(),
        identity_auth.clone(),
        identity_directory,
    )))?;
    let object_store = build_object_store(&config.storage).await?;
    capabilities.register(Arc::new(FilesCapability::new(
        database.clone(),
        identity_auth,
        object_store,
    )))?;
    let queue = build_queue(&config.queue, database.as_ref())?;
    capabilities.register(Arc::new(JobsCapability::new(queue)))?;
    capabilities.register(Arc::new(EventsCapability::new(database.clone())))?;

    let address = format!("{}:{}", config.http.host, config.http.port);
    let state = AppState {
        database,
        admin_auth,
        ops,
    };
    let app = build_router(state, &capabilities);
    let listener = tokio::net::TcpListener::bind(&address).await?;
    tracing::info!(%address, "Boson server listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
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

fn build_router(state: AppState, capabilities: &CapabilityRegistry) -> Router {
    let admin = capabilities
        .admin_router()
        .route_layer(middleware::from_fn_with_state(state.clone(), require_admin));
    let core = Router::new()
        .route("/", get(root))
        .route("/healthz", get(liveness))
        .route("/readyz", get(readiness))
        .with_state(state.clone());

    Router::new()
        .merge(core)
        .nest("/v1", capabilities.app_router())
        .nest("/admin/v1", admin)
        .layer(middleware::from_fn_with_state(state.clone(), trace_request))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
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
    fn unknown_queue_provider_fails_closed() {
        let config = QueueConfig {
            provider: "memory".into(),
            ..QueueConfig::default()
        };
        assert!(build_queue(&config, None).is_err());
    }
}
