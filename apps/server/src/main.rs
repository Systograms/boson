use std::{env, sync::Arc, time::Instant};

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
};
use boson_db::Database;
use boson_kernel::{PlatformConfig, RequestContext, init_telemetry};
use boson_ops::{OpsState, RequestTrace};
use chrono::Utc;
use serde::Serialize;
use serde_json::{Value, json};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

#[derive(Clone)]
struct AppState {
    config: Arc<PlatformConfig>,
    database: Option<Database>,
    ops: OpsState,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
    checks: Vec<HealthCheck>,
}

#[derive(Serialize)]
struct HealthCheck {
    name: &'static str,
    status: &'static str,
    message: Option<String>,
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

    let address = format!("{}:{}", config.http.host, config.http.port);
    let state = AppState {
        config,
        database,
        ops: OpsState::default(),
    };
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(&address).await?;
    tracing::info!(%address, "Boson server listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

fn build_router(state: AppState) -> Router {
    let admin = Router::new()
        .route("/health", get(admin_health))
        .route("/overview", get(admin_overview))
        .route("/requests", get(admin_requests))
        .route("/config", get(admin_config))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_admin));

    Router::new()
        .route("/", get(root))
        .route("/healthz", get(liveness))
        .route("/readyz", get(readiness))
        .nest("/admin/v1", admin)
        .layer(middleware::from_fn_with_state(state.clone(), trace_request))
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
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

async fn admin_health(State(state): State<AppState>) -> Json<HealthResponse> {
    let database = match &state.database {
        Some(database) => match database.ping().await {
            Ok(()) => HealthCheck {
                name: "postgres",
                status: "ok",
                message: None,
            },
            Err(error) => HealthCheck {
                name: "postgres",
                status: "down",
                message: Some(error.to_string()),
            },
        },
        None => HealthCheck {
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
                HealthCheck {
                    name: "worker",
                    status: "ok",
                    message: None,
                }
            }
            Ok(_) => HealthCheck {
                name: "worker",
                status: "down",
                message: Some("no recent worker heartbeat".into()),
            },
            Err(error) => HealthCheck {
                name: "worker",
                status: "down",
                message: Some(error.to_string()),
            },
        },
        None => HealthCheck {
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

async fn admin_overview(State(state): State<AppState>) -> Json<Value> {
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

async fn admin_requests(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "data": state.ops.traces().await }))
}

async fn admin_config(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "snapshot_id": state.config.snapshot_id(),
        "effective": state.config.redacted(),
        "read_only": true
    }))
}

async fn require_admin(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let expected = &state.config.admin.bootstrap_token;
    let supplied = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));

    if expected.is_empty() || supplied != Some(expected.as_str()) {
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
    }
    next.run(request).await
}

async fn trace_request(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let context = RequestContext::new();
    let request_id = context.request_id.to_string();
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
            started_at: Utc::now(),
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
