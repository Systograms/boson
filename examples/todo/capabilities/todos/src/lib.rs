//! Example Todo capability that exercises the public Boson Capability SDK.

use std::{path::PathBuf, sync::Arc};

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    middleware,
    response::{IntoResponse, Response},
    routing::{get, patch, post},
};
use boson_sdk::prelude::*;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Deserialize)]
struct TodosConfig {
    #[serde(default = "default_max_todos")]
    max_todos: u32,
}

fn default_max_todos() -> u32 {
    100
}

#[derive(Clone)]
struct TodosState {
    database: Option<Database>,
    auth: IdentityAuth,
    max_todos: u32,
}

#[derive(Clone)]
pub struct TodosCapability {
    state: TodosState,
}

impl TodosCapability {
    /// Builds the todo capability from runtime services and namespaced config.
    ///
    /// # Errors
    ///
    /// Returns an error when `capabilities.todos` cannot be deserialized.
    pub fn new(
        database: Option<Database>,
        auth: IdentityAuth,
        config: &PlatformConfig,
    ) -> anyhow::Result<Self> {
        let todos = config
            .capability_config::<TodosConfig>("todos")?
            .unwrap_or(TodosConfig {
                max_todos: default_max_todos(),
            });
        Ok(Self {
            state: TodosState {
                database,
                auth,
                max_todos: todos.max_todos,
            },
        })
    }
}

impl Capability for TodosCapability {
    fn descriptor(&self) -> CapabilityDescriptor {
        CapabilityDescriptor {
            name: "todos",
            version: env!("CARGO_PKG_VERSION"),
            dependencies: &["identity"],
        }
    }

    fn scopes(&self) -> &'static [&'static str] {
        &["todos:read"]
    }

    fn migrations(&self) -> Option<MigrationSet> {
        Some(MigrationSet {
            owner: "todos",
            path: PathBuf::from("capabilities/todos/migrations"),
        })
    }

    fn app_router(&self) -> Router {
        Router::new()
            .route("/todos", post(create_todo).get(list_todos))
            .route("/todos/{id}", patch(update_todo))
            .route_layer(middleware::from_fn_with_state(
                self.state.auth.clone(),
                user_auth_middleware,
            ))
            .with_state(self.state.clone())
    }

    fn admin_router(&self) -> Router {
        Router::new()
            .route("/todos", get(admin_list_todos))
            .with_state(self.state.clone())
    }

    fn event_consumers(&self) -> Vec<Arc<dyn EventConsumer>> {
        vec![Arc::new(TodoCreatedLogger)]
    }

    fn health_checks(&self) -> Vec<Arc<dyn HealthCheck>> {
        vec![Arc::new(TodosHealth)]
    }
}

#[derive(Debug, Deserialize)]
struct CreateTodoRequest {
    title: String,
}

#[derive(Debug, Deserialize)]
struct UpdateTodoRequest {
    completed: bool,
}

#[derive(Debug, Serialize)]
struct Todo {
    id: Uuid,
    owner_id: Uuid,
    title: String,
    completed: bool,
}

#[derive(Debug, Error)]
enum TodosError {
    #[error("invalid request: {0}")]
    Invalid(String),
    #[error("todo not found")]
    NotFound,
    #[error("todos service unavailable: {0}")]
    Unavailable(String),
    #[error("missing required scope `{0}`")]
    Forbidden(&'static str),
}

impl IntoResponse for TodosError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            Self::Invalid(_) => (StatusCode::BAD_REQUEST, "todos.invalid"),
            Self::NotFound => (StatusCode::NOT_FOUND, "todos.not_found"),
            Self::Unavailable(_) => (StatusCode::SERVICE_UNAVAILABLE, "todos.unavailable"),
            Self::Forbidden(_) => (StatusCode::FORBIDDEN, "todos.forbidden"),
        };
        api_error(status, code, self.to_string())
    }
}

async fn create_todo(
    State(state): State<TodosState>,
    Extension(user): Extension<AuthenticatedUser>,
    Extension(context): Extension<RequestContext>,
    Json(input): Json<CreateTodoRequest>,
) -> Result<(StatusCode, Json<Todo>), TodosError> {
    let title = input.title.trim();
    if title.is_empty() {
        return Err(TodosError::Invalid("title is required".into()));
    }
    let database = require_database(&state)?;
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM todos.todos WHERE owner_id = $1 AND deleted_at IS NULL",
    )
    .bind(user.user_id)
    .fetch_one(database.pool())
    .await
    .map_err(|error| unavailable(&error))?;
    if count >= i64::from(state.max_todos) {
        return Err(TodosError::Invalid(format!(
            "at most {} todos are allowed",
            state.max_todos
        )));
    }

    let id = Uuid::now_v7();
    let mut tx = database
        .pool()
        .begin()
        .await
        .map_err(|error| unavailable(&error))?;
    sqlx::query("INSERT INTO todos.todos (id, owner_id, title) VALUES ($1, $2, $3)")
        .bind(id)
        .bind(user.user_id)
        .bind(title)
        .execute(&mut *tx)
        .await
        .map_err(|error| unavailable(&error))?;
    publish_in_tx(
        &mut tx,
        &EventEnvelope::new(
            "todos.todo_created.v1",
            json!({
                "id": id,
                "owner_id": user.user_id,
                "request_id": context.request_id
            }),
        ),
    )
    .await
    .map_err(|error| unavailable(&error))?;
    tx.commit()
        .await
        .map_err(|error| unavailable(&error))?;

    Ok((
        StatusCode::CREATED,
        Json(Todo {
            id,
            owner_id: user.user_id,
            title: title.to_owned(),
            completed: false,
        }),
    ))
}

async fn list_todos(
    State(state): State<TodosState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Result<Json<Value>, TodosError> {
    let database = require_database(&state)?;
    let rows = sqlx::query(
        "SELECT id, owner_id, title, completed FROM todos.todos
         WHERE owner_id = $1 AND deleted_at IS NULL
         ORDER BY created_at DESC",
    )
    .bind(user.user_id)
    .fetch_all(database.pool())
    .await
    .map_err(|error| unavailable(&error))?;
    let todos = rows
        .iter()
        .map(todo_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(json!({ "data": todos })))
}

async fn update_todo(
    State(state): State<TodosState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateTodoRequest>,
) -> Result<Json<Todo>, TodosError> {
    let database = require_database(&state)?;
    let row = sqlx::query(
        "UPDATE todos.todos
         SET completed = $1, updated_at = now()
         WHERE id = $2 AND owner_id = $3 AND deleted_at IS NULL
         RETURNING id, owner_id, title, completed",
    )
    .bind(input.completed)
    .bind(id)
    .bind(user.user_id)
    .fetch_optional(database.pool())
    .await
    .map_err(|error| unavailable(&error))?
    .ok_or(TodosError::NotFound)?;
    Ok(Json(todo_from_row(&row)?))
}

async fn admin_list_todos(
    State(state): State<TodosState>,
    Extension(principal): Extension<AdminPrincipal>,
) -> Result<Json<Value>, TodosError> {
    require_scope(&principal, "todos:read", TodosError::Forbidden)?;
    let database = require_database(&state)?;
    let rows = sqlx::query(
        "SELECT id, owner_id, title, completed FROM todos.todos
         WHERE deleted_at IS NULL
         ORDER BY created_at DESC
         LIMIT 100",
    )
    .fetch_all(database.pool())
    .await
    .map_err(|error| unavailable(&error))?;
    let todos = rows
        .iter()
        .map(todo_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(json!({ "data": todos })))
}

fn require_database(state: &TodosState) -> Result<&Database, TodosError> {
    state
        .database
        .as_ref()
        .ok_or_else(|| TodosError::Unavailable("database is not configured".into()))
}

fn unavailable(error: &impl ToString) -> TodosError {
    TodosError::Unavailable(error.to_string())
}

fn todo_from_row(row: &sqlx::postgres::PgRow) -> Result<Todo, TodosError> {
    Ok(Todo {
        id: row.try_get("id").map_err(|error| unavailable(&error))?,
        owner_id: row
            .try_get("owner_id")
            .map_err(|error| unavailable(&error))?,
        title: row.try_get("title").map_err(|error| unavailable(&error))?,
        completed: row
            .try_get("completed")
            .map_err(|error| unavailable(&error))?,
    })
}

struct TodoCreatedLogger;

#[async_trait]
impl EventConsumer for TodoCreatedLogger {
    fn name(&self) -> &'static str {
        "todos.todo_created_logger"
    }

    fn topic(&self) -> &'static str {
        "todos.todo_created.v1"
    }

    async fn handle(&self, event: &EventEnvelope) -> Result<(), EventError> {
        tracing::info!(topic = %event.topic, id = %event.id, "todo created");
        Ok(())
    }
}

struct TodosHealth;

#[async_trait]
impl HealthCheck for TodosHealth {
    async fn check(&self) -> HealthStatus {
        HealthStatus {
            component: "todos".into(),
            healthy: true,
            message: None,
            latency_ms: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_and_scopes_are_stable() {
        let capability = TodosCapability {
            state: TodosState {
                database: None,
                auth: IdentityAuth::new(&AuthConfig {
                    issuer: "test".into(),
                    jwt_secret: "secret".into(),
                    ..AuthConfig::default()
                }),
                max_todos: 10,
            },
        };
        assert_eq!(capability.descriptor().name, "todos");
        assert_eq!(capability.scopes(), &["todos:read"]);
        assert!(capability.migrations().is_some());
        assert_eq!(capability.event_consumers().len(), 1);
        assert_eq!(capability.health_checks().len(), 1);
    }
}
