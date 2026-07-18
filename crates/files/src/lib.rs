//! End-user file storage: metadata in `PostgreSQL`, bytes behind an
//! [`ObjectStore`] port.
//!
//! Uploads are direct: clients POST bytes with filename and content-type
//! headers, and download through the API. Object keys and filesystem details
//! never leave this capability. Uploads and deletes emit versioned outbox
//! events in the same transaction that changes the metadata row.

use std::sync::Arc;

use axum::{
    Extension, Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, Path, Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use boson_admin::AdminPrincipal;
use boson_capability::{Capability, CapabilityDescriptor};
use boson_db::Database;
use boson_identity::{AuthenticatedUser, IdentityAuth};
use boson_ports::{HealthCheck, ObjectMetadata, ObjectStore, PortError};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::json;
use sqlx::Row;
use thiserror::Error;
use uuid::Uuid;

/// Header carrying the client-supplied filename on direct uploads.
pub const FILENAME_HEADER: &str = "x-boson-filename";
const MAX_UPLOAD_BYTES: usize = 32 * 1024 * 1024;
const MAX_FILENAME_CHARS: usize = 255;

#[derive(Clone)]
struct FilesState {
    database: Option<Database>,
    auth: IdentityAuth,
    store: Arc<dyn ObjectStore>,
}

#[derive(Clone)]
pub struct FilesCapability {
    state: FilesState,
}

impl FilesCapability {
    #[must_use]
    pub fn new(
        database: Option<Database>,
        auth: IdentityAuth,
        store: Arc<dyn ObjectStore>,
    ) -> Self {
        Self {
            state: FilesState {
                database,
                auth,
                store,
            },
        }
    }
}

impl Capability for FilesCapability {
    fn descriptor(&self) -> CapabilityDescriptor {
        CapabilityDescriptor {
            name: "files",
            version: env!("CARGO_PKG_VERSION"),
            dependencies: &["identity"],
        }
    }

    fn app_router(&self) -> Router {
        Router::new()
            .route("/files", post(upload_file).get(list_files))
            .route("/files/{file_id}/content", get(download_file))
            .route("/files/{file_id}", delete(delete_file))
            .layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES))
            .route_layer(middleware::from_fn_with_state(
                self.state.clone(),
                require_user,
            ))
            .with_state(self.state.clone())
    }

    fn admin_router(&self) -> Router {
        Router::new()
            .route("/files", get(admin_list_files))
            .with_state(self.state.clone())
    }

    fn health_checks(&self) -> Vec<Arc<dyn HealthCheck>> {
        vec![Arc::clone(&self.state.store) as Arc<dyn HealthCheck>]
    }
}

#[derive(Debug, Error)]
pub enum FilesError {
    #[error("invalid or expired credentials")]
    Unauthorized,
    #[error("missing required scope `{0}`")]
    Forbidden(&'static str),
    #[error("file not found")]
    NotFound,
    #[error("invalid request: {0}")]
    Invalid(String),
    #[error("files service unavailable: {0}")]
    Unavailable(String),
}

impl IntoResponse for FilesError {
    fn into_response(self) -> Response {
        let (status, code) = match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "files.unauthorized"),
            Self::Forbidden(_) => (StatusCode::FORBIDDEN, "files.forbidden"),
            Self::NotFound => (StatusCode::NOT_FOUND, "files.not_found"),
            Self::Invalid(_) => (StatusCode::BAD_REQUEST, "files.invalid"),
            Self::Unavailable(_) => (StatusCode::SERVICE_UNAVAILABLE, "files.unavailable"),
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

impl From<PortError> for FilesError {
    fn from(error: PortError) -> Self {
        match error {
            PortError::NotFound => Self::NotFound,
            PortError::Invalid(message) => Self::Invalid(message),
            PortError::Unavailable(message) | PortError::Provider(message) => {
                Self::Unavailable(message)
            }
        }
    }
}

/// Public file representation. Never carries object keys or disk paths.
#[derive(Debug, Serialize)]
struct FileDto {
    id: Uuid,
    filename: String,
    content_type: String,
    size_bytes: i64,
    status: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// Admin file representation: adds the owner, still no storage internals.
#[derive(Debug, Serialize)]
struct AdminFileDto {
    id: Uuid,
    owner_id: Uuid,
    filename: String,
    content_type: String,
    size_bytes: i64,
    status: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
}

async fn upload_file(
    State(state): State<FilesState>,
    Extension(user): Extension<AuthenticatedUser>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<FileDto>), FilesError> {
    let filename = required_header(&headers, FILENAME_HEADER)?;
    validate_filename(&filename)?;
    let content_type = required_header(&headers, header::CONTENT_TYPE.as_str())?;
    let size_bytes = i64::try_from(body.len())
        .map_err(|_| FilesError::Invalid("upload exceeds the maximum size".into()))?;
    let database = require_database(&state)?;

    let file_id = Uuid::now_v7();
    let object_key = object_key(user.user_id, file_id);

    // Phase 1: reserve the metadata row so a crash cannot orphan bytes
    // invisibly; pending rows are never listed or downloadable.
    sqlx::query(
        "INSERT INTO files.files
         (id, owner_id, object_key, filename, content_type, size_bytes, status)
         VALUES ($1, $2, $3, $4, $5, $6, 'pending')",
    )
    .bind(file_id)
    .bind(user.user_id)
    .bind(&object_key)
    .bind(&filename)
    .bind(&content_type)
    .bind(size_bytes)
    .execute(database.pool())
    .await
    .map_err(unavailable)?;

    // Phase 2: persist the bytes.
    let put = state
        .store
        .put(
            &object_key,
            body,
            ObjectMetadata {
                content_type: Some(content_type.clone()),
                custom: [("filename".to_owned(), filename.clone())].into(),
            },
        )
        .await;
    if let Err(error) = put {
        let _ = sqlx::query("DELETE FROM files.files WHERE id = $1")
            .bind(file_id)
            .execute(database.pool())
            .await;
        return Err(error.into());
    }

    // Phase 3: publish visibility and the event together.
    let mut transaction = database.pool().begin().await.map_err(unavailable)?;
    let row = sqlx::query(
        "UPDATE files.files
         SET status = 'ready', updated_at = now()
         WHERE id = $1
         RETURNING created_at, updated_at",
    )
    .bind(file_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(unavailable)?;
    insert_outbox_event(
        &mut transaction,
        "files.file_uploaded.v1",
        json!({
            "file_id": file_id,
            "owner_id": user.user_id,
            "filename": filename,
            "content_type": content_type,
            "size_bytes": size_bytes
        }),
    )
    .await
    .map_err(unavailable)?;
    transaction.commit().await.map_err(unavailable)?;

    Ok((
        StatusCode::CREATED,
        Json(FileDto {
            id: file_id,
            filename,
            content_type,
            size_bytes,
            status: "ready".into(),
            created_at: row.try_get("created_at").map_err(unavailable)?,
            updated_at: row.try_get("updated_at").map_err(unavailable)?,
        }),
    ))
}

async fn list_files(
    State(state): State<FilesState>,
    Extension(user): Extension<AuthenticatedUser>,
) -> Result<Json<serde_json::Value>, FilesError> {
    let database = require_database(&state)?;
    let rows = sqlx::query(
        "SELECT id, filename, content_type, size_bytes, status, created_at, updated_at
         FROM files.files
         WHERE owner_id = $1 AND deleted_at IS NULL AND status = 'ready'
         ORDER BY created_at DESC
         LIMIT 200",
    )
    .bind(user.user_id)
    .fetch_all(database.pool())
    .await
    .map_err(unavailable)?;
    let files = rows
        .iter()
        .map(file_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(json!({ "data": files })))
}

async fn download_file(
    State(state): State<FilesState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(file_id): Path<Uuid>,
) -> Result<Response, FilesError> {
    let database = require_database(&state)?;
    let row = sqlx::query(
        "SELECT object_key, filename, content_type
         FROM files.files
         WHERE id = $1 AND owner_id = $2 AND deleted_at IS NULL AND status = 'ready'",
    )
    .bind(file_id)
    .bind(user.user_id)
    .fetch_optional(database.pool())
    .await
    .map_err(unavailable)?
    .ok_or(FilesError::NotFound)?;

    let object_key: String = row.try_get("object_key").map_err(unavailable)?;
    let filename: String = row.try_get("filename").map_err(unavailable)?;
    let content_type: String = row.try_get("content_type").map_err(unavailable)?;
    let object = state.store.get(&object_key).await?;

    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_DISPOSITION, content_disposition(&filename))
        .body(Body::from(object.bytes))
        .map_err(unavailable)
}

async fn delete_file(
    State(state): State<FilesState>,
    Extension(user): Extension<AuthenticatedUser>,
    Path(file_id): Path<Uuid>,
) -> Result<StatusCode, FilesError> {
    let database = require_database(&state)?;
    let mut transaction = database.pool().begin().await.map_err(unavailable)?;
    let row = sqlx::query(
        "UPDATE files.files
         SET deleted_at = now(), updated_at = now()
         WHERE id = $1 AND owner_id = $2 AND deleted_at IS NULL
         RETURNING object_key",
    )
    .bind(file_id)
    .bind(user.user_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(unavailable)?
    .ok_or(FilesError::NotFound)?;
    let object_key: String = row.try_get("object_key").map_err(unavailable)?;
    insert_outbox_event(
        &mut transaction,
        "files.file_deleted.v1",
        json!({ "file_id": file_id, "owner_id": user.user_id }),
    )
    .await
    .map_err(unavailable)?;
    transaction.commit().await.map_err(unavailable)?;

    // Metadata is authoritative; a failed byte cleanup only leaks storage.
    if let Err(error) = state.store.delete(&object_key).await {
        tracing::warn!(%file_id, %error, "failed to remove deleted file bytes");
    }
    Ok(StatusCode::NO_CONTENT)
}

async fn admin_list_files(
    State(state): State<FilesState>,
    Extension(principal): Extension<AdminPrincipal>,
) -> Result<Json<serde_json::Value>, FilesError> {
    if !principal.allows("storage:read") {
        return Err(FilesError::Forbidden("storage:read"));
    }
    let database = require_database(&state)?;
    let rows = sqlx::query(
        "SELECT id, owner_id, filename, content_type, size_bytes, status,
                created_at, updated_at, deleted_at
         FROM files.files
         ORDER BY created_at DESC
         LIMIT 200",
    )
    .fetch_all(database.pool())
    .await
    .map_err(unavailable)?;
    let files = rows
        .iter()
        .map(admin_file_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(json!({ "data": files })))
}

async fn require_user(State(state): State<FilesState>, request: Request, next: Next) -> Response {
    let supplied = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let Some(token) = supplied else {
        return FilesError::Unauthorized.into_response();
    };
    match state.auth.validate(token) {
        Ok(user) => {
            let mut request = request;
            request.extensions_mut().insert(user);
            next.run(request).await
        }
        Err(_) => FilesError::Unauthorized.into_response(),
    }
}

/// Builds the logical object key for a file. Keys never contain
/// client-controlled input, so they always pass adapter validation.
fn object_key(owner_id: Uuid, file_id: Uuid) -> String {
    format!("users/{owner_id}/{file_id}")
}

fn required_header(headers: &HeaderMap, name: &str) -> Result<String, FilesError> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| FilesError::Invalid(format!("the `{name}` header is required")))
}

fn validate_filename(filename: &str) -> Result<(), FilesError> {
    if filename.chars().count() > MAX_FILENAME_CHARS {
        return Err(FilesError::Invalid(format!(
            "filename must be at most {MAX_FILENAME_CHARS} characters"
        )));
    }
    if filename.contains(['/', '\\']) || filename.chars().any(char::is_control) {
        return Err(FilesError::Invalid(
            "filename must not contain path separators or control characters".into(),
        ));
    }
    if filename == "." || filename == ".." {
        return Err(FilesError::Invalid("filename is not allowed".into()));
    }
    Ok(())
}

/// Renders a Content-Disposition value that cannot break out of the quoted
/// string, falling back to ASCII for exotic names.
fn content_disposition(filename: &str) -> String {
    let safe: String = filename
        .chars()
        .map(|character| {
            if character == '"' || character == '\\' || !character.is_ascii() {
                '_'
            } else {
                character
            }
        })
        .collect();
    format!("attachment; filename=\"{safe}\"")
}

fn require_database(state: &FilesState) -> Result<&Database, FilesError> {
    state
        .database
        .as_ref()
        .ok_or_else(|| FilesError::Unavailable("PostgreSQL is disabled".into()))
}

#[allow(clippy::needless_pass_by_value)]
fn unavailable(error: impl ToString) -> FilesError {
    FilesError::Unavailable(error.to_string())
}

fn file_from_row(row: &sqlx::postgres::PgRow) -> Result<FileDto, FilesError> {
    Ok(FileDto {
        id: row.try_get("id").map_err(unavailable)?,
        filename: row.try_get("filename").map_err(unavailable)?,
        content_type: row.try_get("content_type").map_err(unavailable)?,
        size_bytes: row.try_get("size_bytes").map_err(unavailable)?,
        status: row.try_get("status").map_err(unavailable)?,
        created_at: row.try_get("created_at").map_err(unavailable)?,
        updated_at: row.try_get("updated_at").map_err(unavailable)?,
    })
}

fn admin_file_from_row(row: &sqlx::postgres::PgRow) -> Result<AdminFileDto, FilesError> {
    Ok(AdminFileDto {
        id: row.try_get("id").map_err(unavailable)?,
        owner_id: row.try_get("owner_id").map_err(unavailable)?,
        filename: row.try_get("filename").map_err(unavailable)?,
        content_type: row.try_get("content_type").map_err(unavailable)?,
        size_bytes: row.try_get("size_bytes").map_err(unavailable)?,
        status: row.try_get("status").map_err(unavailable)?,
        created_at: row.try_get("created_at").map_err(unavailable)?,
        updated_at: row.try_get("updated_at").map_err(unavailable)?,
        deleted_at: row.try_get("deleted_at").map_err(unavailable)?,
    })
}

async fn insert_outbox_event(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    topic: &str,
    payload: serde_json::Value,
) -> Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO kernel.outbox
         (id, topic, payload, occurred_at)
         VALUES ($1, $2, $3, now())",
    )
    .bind(Uuid::now_v7())
    .bind(topic)
    .bind(payload)
    .execute(&mut **transaction)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_keys_are_logical_and_stable() {
        let owner = Uuid::nil();
        let file = Uuid::nil();
        assert_eq!(object_key(owner, file), format!("users/{owner}/{file}"));
        assert!(!object_key(owner, file).starts_with('/'));
    }

    #[test]
    fn hostile_filenames_are_rejected() {
        for filename in ["a/b.txt", "..\\evil", "..", ".", "line\nbreak"] {
            assert!(
                validate_filename(filename).is_err(),
                "accepted `{filename}`"
            );
        }
        let long = "x".repeat(MAX_FILENAME_CHARS + 1);
        assert!(validate_filename(&long).is_err());
        assert!(validate_filename("report (final) v2.pdf").is_ok());
    }

    #[test]
    fn content_disposition_cannot_escape_quotes() {
        assert_eq!(
            content_disposition("a\"b\\c.txt"),
            "attachment; filename=\"a_b_c.txt\""
        );
        assert_eq!(
            content_disposition("résumé.pdf"),
            "attachment; filename=\"r_sum_.pdf\""
        );
    }
}
