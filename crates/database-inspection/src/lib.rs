//! Provider-neutral, read-only database inspection Admin capability.

use std::sync::Arc;

use axum::{
    Extension, Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use boson_admin::AdminPrincipal;
use boson_capability::{Capability, CapabilityDescriptor};
use boson_ports::{
    ColumnFilter, DatabaseInspector, DatabaseInspectorCapabilities, PortError, RowPage, RowQuery,
    TableRef, TableSchema, TableSummary,
};
use serde::Deserialize;
use serde_json::json;
use thiserror::Error;

const DATABASE_READ_SCOPE: &str = "database:read";

#[derive(Clone)]
struct DatabaseInspectionState {
    inspector: Option<Arc<dyn DatabaseInspector>>,
}

#[derive(Clone)]
pub struct DatabaseInspectionCapability {
    state: DatabaseInspectionState,
}

impl DatabaseInspectionCapability {
    #[must_use]
    pub fn new(inspector: Option<Arc<dyn DatabaseInspector>>) -> Self {
        Self {
            state: DatabaseInspectionState { inspector },
        }
    }
}

impl Capability for DatabaseInspectionCapability {
    fn descriptor(&self) -> CapabilityDescriptor {
        CapabilityDescriptor {
            name: "database-inspection",
            version: env!("CARGO_PKG_VERSION"),
            dependencies: &["admin"],
        }
    }

    fn admin_router(&self) -> Router {
        Router::new()
            .route("/database", get(capabilities))
            .route("/database/tables", get(list_tables))
            .route("/database/tables/{namespace}/{table}", get(describe_table))
            .route("/database/tables/{namespace}/{table}/rows", get(query_rows))
            .with_state(self.state.clone())
    }
}

#[derive(Debug, Deserialize)]
struct RowsParams {
    limit: Option<u32>,
    cursor: Option<String>,
    column: Option<String>,
    value: Option<String>,
}

async fn capabilities(
    State(state): State<DatabaseInspectionState>,
    Extension(principal): Extension<AdminPrincipal>,
) -> Result<Json<DatabaseInspectorCapabilities>, InspectionError> {
    require(&principal)?;
    Ok(Json(inspector(&state)?.capabilities()))
}

async fn list_tables(
    State(state): State<DatabaseInspectionState>,
    Extension(principal): Extension<AdminPrincipal>,
) -> Result<Json<serde_json::Value>, InspectionError> {
    require(&principal)?;
    let tables: Vec<TableSummary> = inspector(&state)?.list_tables().await?;
    Ok(Json(json!({ "data": tables })))
}

async fn describe_table(
    State(state): State<DatabaseInspectionState>,
    Extension(principal): Extension<AdminPrincipal>,
    Path((namespace, table)): Path<(String, String)>,
) -> Result<Json<TableSchema>, InspectionError> {
    require(&principal)?;
    let table = TableRef {
        namespace,
        name: table,
    };
    Ok(Json(inspector(&state)?.describe_table(&table).await?))
}

async fn query_rows(
    State(state): State<DatabaseInspectionState>,
    Extension(principal): Extension<AdminPrincipal>,
    Path((namespace, table)): Path<(String, String)>,
    Query(params): Query<RowsParams>,
) -> Result<Json<RowPage>, InspectionError> {
    require(&principal)?;
    let filters = match (params.column, params.value) {
        (Some(column), Some(value)) => vec![ColumnFilter { column, value }],
        (None, None) => Vec::new(),
        _ => {
            return Err(InspectionError::Invalid(
                "`column` and `value` must be provided together".into(),
            ));
        }
    };
    let request = RowQuery {
        limit: params.limit.unwrap_or(100),
        cursor: params.cursor,
        filters,
    };
    let table = TableRef {
        namespace,
        name: table,
    };
    Ok(Json(inspector(&state)?.query_rows(&table, request).await?))
}

fn inspector(
    state: &DatabaseInspectionState,
) -> Result<&Arc<dyn DatabaseInspector>, InspectionError> {
    state.inspector.as_ref().ok_or(InspectionError::Disabled)
}

fn require(principal: &AdminPrincipal) -> Result<(), InspectionError> {
    if principal.allows(DATABASE_READ_SCOPE) {
        Ok(())
    } else {
        Err(InspectionError::Forbidden)
    }
}

#[derive(Debug, Error)]
enum InspectionError {
    #[error("database inspection is disabled")]
    Disabled,
    #[error("missing required scope `database:read`")]
    Forbidden,
    #[error("database resource not found")]
    NotFound,
    #[error("invalid database inspection request: {0}")]
    Invalid(String),
    #[error("database inspection unavailable: {0}")]
    Unavailable(String),
}

impl From<PortError> for InspectionError {
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

impl IntoResponse for InspectionError {
    fn into_response(self) -> Response {
        let (status, code) = match self {
            Self::Disabled => (
                StatusCode::SERVICE_UNAVAILABLE,
                "database_inspection.disabled",
            ),
            Self::Forbidden => (StatusCode::FORBIDDEN, "database_inspection.forbidden"),
            Self::NotFound => (StatusCode::NOT_FOUND, "database_inspection.not_found"),
            Self::Invalid(_) => (StatusCode::BAD_REQUEST, "database_inspection.invalid"),
            Self::Unavailable(_) => (
                StatusCode::SERVICE_UNAVAILABLE,
                "database_inspection.unavailable",
            ),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_invalid_port_requests_to_bad_requests() {
        let response =
            InspectionError::from(PortError::Invalid("bad filter".into())).into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[test]
    fn requires_database_scope() {
        let principal = AdminPrincipal {
            admin_id: None,
            email: None,
            scopes: vec!["ops:read".into()],
            bootstrap: false,
        };
        assert!(matches!(
            require(&principal),
            Err(InspectionError::Forbidden)
        ));
    }
}
