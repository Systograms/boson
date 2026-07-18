//! Persistent platform administrator identities and scoped API keys.

use std::sync::Arc;

use axum::{
    Extension, Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use boson_capability::{Capability, CapabilityDescriptor};
use boson_db::Database;
use boson_kernel::{AdminConfig, RequestContext};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::Row;
use subtle::ConstantTimeEq;
use thiserror::Error;
use uuid::Uuid;

const DEFAULT_SCOPES: &[&str] = &[
    "admins:read",
    "admins:write",
    "audit:read",
    "identity:read",
    "organizations:read",
    "ops:read",
    "config:read",
    "database:read",
    "events:read",
    "jobs:read",
    "jobs:write",
    "notifications:read",
    "storage:read",
    "storage:write",
];

#[derive(Debug, Clone, Serialize)]
pub struct AdminPrincipal {
    pub admin_id: Option<Uuid>,
    pub email: Option<String>,
    pub scopes: Vec<String>,
    pub bootstrap: bool,
}

impl AdminPrincipal {
    #[must_use]
    pub fn allows(&self, scope: &str) -> bool {
        self.bootstrap || self.scopes.iter().any(|allowed| allowed == scope)
    }
}

#[derive(Clone)]
pub struct AdminAuth {
    database: Option<Database>,
    bootstrap_token: Arc<str>,
}

impl AdminAuth {
    #[must_use]
    pub fn new(database: Option<Database>, config: &AdminConfig) -> Self {
        Self {
            database,
            bootstrap_token: Arc::from(config.bootstrap_token.as_str()),
        }
    }

    /// Authenticates a break-glass bootstrap token or persistent Admin API key.
    ///
    /// # Errors
    ///
    /// Returns [`AdminError::Unauthorized`] for invalid credentials and
    /// [`AdminError::Unavailable`] when persistent authentication cannot query
    /// `PostgreSQL`.
    pub async fn authenticate(&self, token: &str) -> Result<AdminPrincipal, AdminError> {
        if !self.bootstrap_token.is_empty()
            && self
                .bootstrap_token
                .as_bytes()
                .ct_eq(token.as_bytes())
                .into()
        {
            return Ok(AdminPrincipal {
                admin_id: None,
                email: None,
                scopes: vec!["*".into()],
                bootstrap: true,
            });
        }

        let database = self.database.as_ref().ok_or(AdminError::Unauthorized)?;
        let token_hash = hash_token(token);
        let row = sqlx::query(
            "SELECT u.id, u.email, k.id AS key_id, k.scopes
             FROM admin.api_keys k
             JOIN admin.users u ON u.id = k.admin_id
             WHERE k.token_hash = $1
               AND k.revoked_at IS NULL
               AND (k.expires_at IS NULL OR k.expires_at > now())
               AND u.disabled_at IS NULL",
        )
        .bind(token_hash)
        .fetch_optional(database.pool())
        .await
        .map_err(|error| AdminError::Unavailable(error.to_string()))?
        .ok_or(AdminError::Unauthorized)?;

        let key_id: Uuid = row
            .try_get("key_id")
            .map_err(|error| AdminError::Unavailable(error.to_string()))?;
        sqlx::query("UPDATE admin.api_keys SET last_used_at = now() WHERE id = $1")
            .bind(key_id)
            .execute(database.pool())
            .await
            .map_err(|error| AdminError::Unavailable(error.to_string()))?;

        Ok(AdminPrincipal {
            admin_id: Some(
                row.try_get("id")
                    .map_err(|error| AdminError::Unavailable(error.to_string()))?,
            ),
            email: Some(
                row.try_get("email")
                    .map_err(|error| AdminError::Unavailable(error.to_string()))?,
            ),
            scopes: row
                .try_get("scopes")
                .map_err(|error| AdminError::Unavailable(error.to_string()))?,
            bootstrap: false,
        })
    }
}

#[derive(Clone)]
struct AdminState {
    database: Option<Database>,
}

#[derive(Clone)]
pub struct AdminCapability {
    state: AdminState,
}

impl AdminCapability {
    #[must_use]
    pub fn new(database: Option<Database>) -> Self {
        Self {
            state: AdminState { database },
        }
    }
}

impl Capability for AdminCapability {
    fn descriptor(&self) -> CapabilityDescriptor {
        CapabilityDescriptor {
            name: "admin",
            version: env!("CARGO_PKG_VERSION"),
            dependencies: &["ops"],
        }
    }

    fn admin_router(&self) -> Router {
        Router::new()
            .route("/admin-session", get(current_session))
            .route("/admins", get(list_admins).post(create_admin))
            .route("/admins/{admin_id}/keys", post(create_api_key))
            .route("/admin-keys", get(list_api_keys))
            .with_state(self.state.clone())
    }
}

#[derive(Debug, Error)]
pub enum AdminError {
    #[error("invalid or expired Admin credential")]
    Unauthorized,
    #[error("missing required scope `{0}`")]
    Forbidden(&'static str),
    #[error("Admin resource not found")]
    NotFound,
    #[error("invalid request: {0}")]
    Invalid(String),
    #[error("Admin service unavailable: {0}")]
    Unavailable(String),
    #[error("Admin resource already exists")]
    Conflict,
}

impl IntoResponse for AdminError {
    fn into_response(self) -> Response {
        let (status, code) = match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "admin.unauthorized"),
            Self::Forbidden(_) => (StatusCode::FORBIDDEN, "admin.forbidden"),
            Self::NotFound => (StatusCode::NOT_FOUND, "admin.not_found"),
            Self::Invalid(_) => (StatusCode::BAD_REQUEST, "admin.invalid"),
            Self::Unavailable(_) => (StatusCode::SERVICE_UNAVAILABLE, "admin.unavailable"),
            Self::Conflict => (StatusCode::CONFLICT, "admin.conflict"),
        };
        (
            status,
            Json(json!({
                "error": {
                    "code": code,
                    "message": self.to_string()
                }
            })),
        )
            .into_response()
    }
}

#[derive(Debug, Serialize)]
struct AdminUser {
    id: Uuid,
    email: String,
    display_name: String,
    disabled_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct ApiKey {
    id: Uuid,
    admin_id: Uuid,
    name: String,
    token_prefix: String,
    scopes: Vec<String>,
    last_used_at: Option<DateTime<Utc>>,
    expires_at: Option<DateTime<Utc>>,
    revoked_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct CreateAdminRequest {
    email: String,
    display_name: String,
    key_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateKeyRequest {
    name: String,
    scopes: Option<Vec<String>>,
}

#[derive(Debug, Serialize)]
struct IssuedCredential {
    admin: AdminUser,
    key: ApiKey,
    /// Returned exactly once. Boson stores only its SHA-256 hash.
    token: String,
}

async fn current_session(Extension(principal): Extension<AdminPrincipal>) -> Json<AdminPrincipal> {
    Json(principal)
}

async fn list_admins(
    State(state): State<AdminState>,
    Extension(principal): Extension<AdminPrincipal>,
) -> Result<Json<serde_json::Value>, AdminError> {
    require(&principal, "admins:read")?;
    let database = require_database(&state)?;
    let rows = sqlx::query(
        "SELECT id, email, display_name, disabled_at, created_at
         FROM admin.users ORDER BY created_at DESC",
    )
    .fetch_all(database.pool())
    .await
    .map_err(unavailable)?;
    let admins = rows
        .into_iter()
        .map(|row| admin_from_row(&row))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(json!({ "data": admins })))
}

async fn create_admin(
    State(state): State<AdminState>,
    Extension(principal): Extension<AdminPrincipal>,
    Extension(context): Extension<RequestContext>,
    Json(input): Json<CreateAdminRequest>,
) -> Result<(StatusCode, Json<IssuedCredential>), AdminError> {
    require(&principal, "admins:write")?;
    let email = normalize_email(&input.email)?;
    if input.display_name.trim().is_empty() {
        return Err(AdminError::Invalid("display_name is required".into()));
    }
    let database = require_database(&state)?;
    let admin_id = Uuid::now_v7();
    let key_id = Uuid::now_v7();
    let token = generate_token();
    let token_hash = hash_token(&token);
    let token_prefix = token.chars().take(20).collect::<String>();
    let key_name = input.key_name.as_deref().unwrap_or("default");
    let scopes = default_scopes();
    let mut transaction = database.pool().begin().await.map_err(unavailable)?;

    let result = sqlx::query(
        "INSERT INTO admin.users (id, email, display_name)
         VALUES ($1, $2, $3)",
    )
    .bind(admin_id)
    .bind(&email)
    .bind(input.display_name.trim())
    .execute(&mut *transaction)
    .await;
    if let Err(error) = result {
        if is_unique_violation(&error) {
            return Err(AdminError::Conflict);
        }
        return Err(unavailable(error));
    }

    sqlx::query(
        "INSERT INTO admin.api_keys
         (id, admin_id, name, token_hash, token_prefix, scopes)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(key_id)
    .bind(admin_id)
    .bind(key_name)
    .bind(token_hash)
    .bind(&token_prefix)
    .bind(&scopes)
    .execute(&mut *transaction)
    .await
    .map_err(unavailable)?;
    insert_admin_event(&mut transaction, admin_id, &email, Some(context.request_id))
        .await
        .map_err(unavailable)?;
    transaction.commit().await.map_err(unavailable)?;

    let now = Utc::now();
    Ok((
        StatusCode::CREATED,
        Json(IssuedCredential {
            admin: AdminUser {
                id: admin_id,
                email,
                display_name: input.display_name.trim().into(),
                disabled_at: None,
                created_at: now,
            },
            key: ApiKey {
                id: key_id,
                admin_id,
                name: key_name.into(),
                token_prefix,
                scopes,
                last_used_at: None,
                expires_at: None,
                revoked_at: None,
                created_at: now,
            },
            token,
        }),
    ))
}

async fn create_api_key(
    State(state): State<AdminState>,
    Extension(principal): Extension<AdminPrincipal>,
    Path(admin_id): Path<Uuid>,
    Json(input): Json<CreateKeyRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), AdminError> {
    require(&principal, "admins:write")?;
    if input.name.trim().is_empty() {
        return Err(AdminError::Invalid("name is required".into()));
    }
    let database = require_database(&state)?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM admin.users WHERE id = $1)")
        .bind(admin_id)
        .fetch_one(database.pool())
        .await
        .map_err(unavailable)?;
    if !exists {
        return Err(AdminError::NotFound);
    }
    let id = Uuid::now_v7();
    let token = generate_token();
    let prefix = token.chars().take(20).collect::<String>();
    let scopes = input.scopes.unwrap_or_else(default_scopes);
    sqlx::query(
        "INSERT INTO admin.api_keys
         (id, admin_id, name, token_hash, token_prefix, scopes)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(admin_id)
    .bind(input.name.trim())
    .bind(hash_token(&token))
    .bind(&prefix)
    .bind(&scopes)
    .execute(database.pool())
    .await
    .map_err(unavailable)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "id": id,
            "admin_id": admin_id,
            "name": input.name.trim(),
            "token_prefix": prefix,
            "scopes": scopes,
            "token": token
        })),
    ))
}

async fn list_api_keys(
    State(state): State<AdminState>,
    Extension(principal): Extension<AdminPrincipal>,
) -> Result<Json<serde_json::Value>, AdminError> {
    require(&principal, "admins:read")?;
    let database = require_database(&state)?;
    let rows = sqlx::query(
        "SELECT id, admin_id, name, token_prefix, scopes, last_used_at,
                expires_at, revoked_at, created_at
         FROM admin.api_keys ORDER BY created_at DESC",
    )
    .fetch_all(database.pool())
    .await
    .map_err(unavailable)?;
    let keys = rows
        .into_iter()
        .map(|row| api_key_from_row(&row))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(json!({ "data": keys })))
}

fn require(principal: &AdminPrincipal, scope: &'static str) -> Result<(), AdminError> {
    if principal.allows(scope) {
        Ok(())
    } else {
        Err(AdminError::Forbidden(scope))
    }
}

fn require_database(state: &AdminState) -> Result<&Database, AdminError> {
    state
        .database
        .as_ref()
        .ok_or_else(|| AdminError::Unavailable("PostgreSQL is disabled".into()))
}

fn normalize_email(email: &str) -> Result<String, AdminError> {
    let email = email.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') || email.len() > 320 {
        return Err(AdminError::Invalid("valid email is required".into()));
    }
    Ok(email)
}

fn default_scopes() -> Vec<String> {
    DEFAULT_SCOPES.iter().map(ToString::to_string).collect()
}

fn generate_token() -> String {
    format!(
        "boson_admin_{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
}

fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

#[allow(clippy::needless_pass_by_value)]
fn unavailable(error: impl ToString) -> AdminError {
    AdminError::Unavailable(error.to_string())
}

fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(sqlx::error::DatabaseError::code)
        .is_some_and(|code| code == "23505")
}

fn admin_from_row(row: &sqlx::postgres::PgRow) -> Result<AdminUser, AdminError> {
    Ok(AdminUser {
        id: row.try_get("id").map_err(unavailable)?,
        email: row.try_get("email").map_err(unavailable)?,
        display_name: row.try_get("display_name").map_err(unavailable)?,
        disabled_at: row.try_get("disabled_at").map_err(unavailable)?,
        created_at: row.try_get("created_at").map_err(unavailable)?,
    })
}

fn api_key_from_row(row: &sqlx::postgres::PgRow) -> Result<ApiKey, AdminError> {
    Ok(ApiKey {
        id: row.try_get("id").map_err(unavailable)?,
        admin_id: row.try_get("admin_id").map_err(unavailable)?,
        name: row.try_get("name").map_err(unavailable)?,
        token_prefix: row.try_get("token_prefix").map_err(unavailable)?,
        scopes: row.try_get("scopes").map_err(unavailable)?,
        last_used_at: row.try_get("last_used_at").map_err(unavailable)?,
        expires_at: row.try_get("expires_at").map_err(unavailable)?,
        revoked_at: row.try_get("revoked_at").map_err(unavailable)?,
        created_at: row.try_get("created_at").map_err(unavailable)?,
    })
}

async fn insert_admin_event(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    admin_id: Uuid,
    email: &str,
    correlation_id: Option<Uuid>,
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO kernel.outbox
         (id, topic, payload, correlation_id, occurred_at)
         VALUES ($1, 'admin.user_created.v1', $2, $3, now())",
    )
    .bind(Uuid::now_v7())
    .bind(json!({ "admin_id": admin_id, "email": email }))
    .bind(correlation_id.map(|id| id.to_string()))
    .execute(&mut **transaction)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_hash_is_not_the_token() {
        let token = generate_token();
        assert!(token.starts_with("boson_admin_"));
        assert_ne!(hash_token(&token), token);
    }

    #[test]
    fn invalid_email_is_rejected() {
        assert!(normalize_email("not-an-email").is_err());
    }
}
