//! Public Capability SDK for Boson application authors.
//!
//! Capability crates should depend on `boson-sdk` instead of reaching into
//! internal platform crates directly. The [`prelude`] module re-exports the
//! common registration, auth, event, and database helpers.

use axum::{
    Json,
    extract::Request,
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

pub use async_trait::async_trait;
pub use axum::{self, Extension, Json as AxumJson, Router, extract::State, routing};
pub use boson_admin::AdminPrincipal;
pub use boson_capability::{
    Capability, CapabilityDescriptor, CapabilityRegistry, JobHandler, MigrationSet,
    RegistrationError, Schedule,
};
pub use boson_db::Database;
pub use boson_events::{EventConsumer, EventEnvelope, EventError};
pub use boson_identity::{AuthenticatedUser, IdentityAuth, IdentityDirectory};
pub use boson_kernel::{AuthConfig, PlatformConfig, RequestContext};
pub use boson_ports::{
    HealthCheck, HealthStatus, JobEnvelope, Mailer, ObjectStore, PortError, Queue,
};
pub use serde_json::{Value, json};
pub use sqlx::{self, Row};
pub use uuid::Uuid;

/// Common imports for capability authors.
pub mod prelude {
    pub use crate::sqlx;
    pub use crate::{
        AdminPrincipal, AuthConfig, AuthenticatedUser, AxumJson, Capability, CapabilityDescriptor,
        Database, EventConsumer, EventEnvelope, EventError, Extension, HealthCheck, HealthStatus,
        IdentityAuth, IdentityDirectory, JobEnvelope, JobHandler, MigrationSet, PlatformConfig,
        PortError, RegistrationError, RequestContext, Router, Row, Schedule, State, Uuid, Value,
        api_error, async_trait, publish_in_tx, require_scope, user_auth_middleware,
    };
    pub use serde_json::json;
}

/// Publishes a versioned event inside an open `PostgreSQL` transaction.
///
/// # Errors
///
/// Returns [`boson_db::DatabaseError`] when the outbox insert fails.
pub async fn publish_in_tx(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    event: &EventEnvelope,
) -> Result<(), boson_db::DatabaseError> {
    Database::publish_in_tx(transaction, event).await
}

/// Convenience constructor for a capability-scoped event envelope.
#[must_use]
pub fn event(topic: impl Into<String>, payload: Value) -> EventEnvelope {
    EventEnvelope::new(topic, payload)
}

/// Standard JSON error body used across Boson APIs.
#[must_use]
pub fn api_error(status: StatusCode, code: &'static str, message: impl Into<String>) -> Response {
    (
        status,
        Json(json!({
            "error": {
                "code": code,
                "message": message.into()
            }
        })),
    )
        .into_response()
}

/// Returns [`Err`] when the Admin principal lacks `scope`.
///
/// # Errors
///
/// Returns the value produced by `forbidden` when the scope is missing.
pub fn require_scope<E>(
    principal: &AdminPrincipal,
    scope: &'static str,
    forbidden: impl FnOnce(&'static str) -> E,
) -> Result<(), E> {
    if principal.allows(scope) {
        Ok(())
    } else {
        Err(forbidden(scope))
    }
}

/// Axum middleware that validates an end-user bearer token.
///
/// Attach with:
/// `route_layer(axum::middleware::from_fn_with_state(auth, user_auth_middleware))`.
pub async fn user_auth_middleware(
    axum::extract::State(auth): axum::extract::State<IdentityAuth>,
    mut request: Request,
    next: Next,
) -> Response {
    let supplied = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let Some(token) = supplied else {
        return api_error(
            StatusCode::UNAUTHORIZED,
            "identity.unauthorized",
            "A valid end-user bearer token is required",
        );
    };
    match auth.validate(token) {
        Ok(user) => {
            request.extensions_mut().insert(user);
            next.run(request).await
        }
        Err(_) => api_error(
            StatusCode::UNAUTHORIZED,
            "identity.unauthorized",
            "A valid end-user bearer token is required",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_scope_accepts_matching_scope() {
        let principal = AdminPrincipal {
            admin_id: None,
            email: None,
            scopes: vec!["todos:read".into()],
            bootstrap: false,
        };
        assert!(require_scope(&principal, "todos:read", |_| ()).is_ok());
        assert!(require_scope(&principal, "todos:write", |_| ()).is_err());
    }
}
