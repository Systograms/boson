//! Event-driven email notifications with durable delivery visibility.
//!
//! Domain capabilities publish notification intent as versioned events.
//! Consumers render provider-neutral [`Email`] values and send through the
//! configured [`Mailer`]. Delivery metadata never stores message bodies or
//! action tokens.

use std::sync::Arc;

use async_trait::async_trait;
use axum::{Extension, Json, Router, extract::State, routing::get};
use boson_admin::AdminPrincipal;
use boson_capability::{Capability, CapabilityDescriptor};
use boson_db::Database;
use boson_events::{EventConsumer, EventEnvelope, EventError};
use boson_ports::{Email, HealthCheck, Mailer};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::Row;
use thiserror::Error;
use uuid::Uuid;

const VERIFICATION_TOPIC: &str = "identity.email_verification_requested.v1";
const PASSWORD_RESET_TOPIC: &str = "identity.password_reset_requested.v1";
const INVITATION_TOPIC: &str = "organizations.invitation_created.v1";

#[derive(Clone)]
pub struct NotificationsCapability {
    database: Option<Database>,
    mailer: Arc<dyn Mailer>,
    from: Arc<str>,
    public_app_url: Arc<str>,
}

impl NotificationsCapability {
    #[must_use]
    pub fn new(
        database: Option<Database>,
        mailer: Arc<dyn Mailer>,
        from: impl Into<Arc<str>>,
        public_app_url: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            database,
            mailer,
            from: from.into(),
            public_app_url: public_app_url.into(),
        }
    }

    fn database(&self) -> Result<&Database, NotificationsError> {
        self.database
            .as_ref()
            .ok_or_else(|| NotificationsError::Unavailable("PostgreSQL is disabled".into()))
    }
}

impl Capability for NotificationsCapability {
    fn descriptor(&self) -> CapabilityDescriptor {
        CapabilityDescriptor {
            name: "notifications",
            version: env!("CARGO_PKG_VERSION"),
            dependencies: &["admin"],
        }
    }

    fn admin_router(&self) -> Router {
        Router::new()
            .route("/notifications", get(list_deliveries))
            .with_state(self.clone())
    }

    fn event_consumers(&self) -> Vec<Arc<dyn EventConsumer>> {
        let Some(database) = &self.database else {
            return Vec::new();
        };
        [VERIFICATION_TOPIC, PASSWORD_RESET_TOPIC, INVITATION_TOPIC]
            .into_iter()
            .map(|topic| {
                Arc::new(EmailConsumer {
                    topic,
                    database: database.clone(),
                    mailer: Arc::clone(&self.mailer),
                    from: Arc::clone(&self.from),
                    public_app_url: Arc::clone(&self.public_app_url),
                }) as Arc<dyn EventConsumer>
            })
            .collect()
    }

    fn health_checks(&self) -> Vec<Arc<dyn HealthCheck>> {
        vec![Arc::clone(&self.mailer) as Arc<dyn HealthCheck>]
    }
}

struct EmailConsumer {
    topic: &'static str,
    database: Database,
    mailer: Arc<dyn Mailer>,
    from: Arc<str>,
    public_app_url: Arc<str>,
}

#[async_trait]
impl EventConsumer for EmailConsumer {
    fn name(&self) -> &'static str {
        match self.topic {
            VERIFICATION_TOPIC => "notifications.email_verification",
            PASSWORD_RESET_TOPIC => "notifications.password_reset",
            INVITATION_TOPIC => "notifications.organization_invitation",
            _ => "notifications.unknown",
        }
    }

    fn topic(&self) -> &'static str {
        self.topic
    }

    async fn handle(&self, event: &EventEnvelope) -> Result<(), EventError> {
        if delivery_was_sent(&self.database, event.id).await? {
            return Ok(());
        }
        let rendered = render_email(
            event,
            self.from.as_ref(),
            self.public_app_url.trim_end_matches('/'),
        )?;
        start_attempt(
            &self.database,
            event.id,
            rendered.kind,
            &rendered.email.to,
            &rendered.email.subject,
        )
        .await?;
        match self.mailer.send(rendered.email).await {
            Ok(()) => {
                finish_attempt(&self.database, event.id, None).await?;
                Ok(())
            }
            Err(error) => {
                let message = error.to_string();
                finish_attempt(&self.database, event.id, Some(&message)).await?;
                Err(EventError::Consumer(message))
            }
        }
    }
}

struct RenderedEmail {
    kind: &'static str,
    email: Email,
}

fn render_email(
    event: &EventEnvelope,
    from: &str,
    public_app_url: &str,
) -> Result<RenderedEmail, EventError> {
    let recipient = required_string(&event.payload, "email")?;
    let token = required_string(&event.payload, "token")?;
    let idempotency_key = event.id.to_string();
    let (kind, subject, text) = match event.topic.as_str() {
        VERIFICATION_TOPIC => (
            "email_verification",
            "Verify your email",
            format!(
                "Verify your Boson account:\n{public_app_url}/auth/verify-email?token={token}\n\nThis link expires shortly and can be used once."
            ),
        ),
        PASSWORD_RESET_TOPIC => (
            "password_reset",
            "Reset your password",
            format!(
                "Reset your Boson password:\n{public_app_url}/auth/reset-password?token={token}\n\nIf you did not request this, ignore this email."
            ),
        ),
        INVITATION_TOPIC => (
            "organization_invitation",
            "You were invited to an organization",
            format!(
                "Accept your Boson organization invitation:\n{public_app_url}/invitations/accept?token={token}\n\nThis invitation can be used once."
            ),
        ),
        _ => return Err(EventError::Invalid("unsupported notification topic".into())),
    };
    Ok(RenderedEmail {
        kind,
        email: Email {
            to: recipient,
            from: from.to_owned(),
            subject: subject.to_owned(),
            text,
            idempotency_key,
        },
    })
}

fn required_string(payload: &Value, key: &str) -> Result<String, EventError> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| EventError::Invalid(format!("notification event requires `{key}`")))
}

async fn delivery_was_sent(database: &Database, event_id: Uuid) -> Result<bool, EventError> {
    sqlx::query_scalar::<_, String>(
        "SELECT status FROM notifications.deliveries WHERE event_id = $1",
    )
    .bind(event_id)
    .fetch_optional(database.pool())
    .await
    .map(|status| status.as_deref() == Some("sent"))
    .map_err(consumer_error)
}

async fn start_attempt(
    database: &Database,
    event_id: Uuid,
    kind: &str,
    recipient: &str,
    subject: &str,
) -> Result<(), EventError> {
    sqlx::query(
        "INSERT INTO notifications.deliveries
         (event_id, kind, recipient, subject, status, attempts, last_attempted_at)
         VALUES ($1, $2, $3, $4, 'pending', 1, now())
         ON CONFLICT (event_id) DO UPDATE
         SET status = 'pending',
             attempts = notifications.deliveries.attempts + 1,
             last_error = NULL,
             last_attempted_at = now()",
    )
    .bind(event_id)
    .bind(kind)
    .bind(recipient)
    .bind(subject)
    .execute(database.pool())
    .await
    .map(|_| ())
    .map_err(consumer_error)
}

async fn finish_attempt(
    database: &Database,
    event_id: Uuid,
    error: Option<&str>,
) -> Result<(), EventError> {
    sqlx::query(
        "UPDATE notifications.deliveries
         SET status = CASE WHEN $2::TEXT IS NULL THEN 'sent' ELSE 'failed' END,
             last_error = $2,
             sent_at = CASE WHEN $2::TEXT IS NULL THEN now() ELSE sent_at END
         WHERE event_id = $1",
    )
    .bind(event_id)
    .bind(error)
    .execute(database.pool())
    .await
    .map(|_| ())
    .map_err(consumer_error)
}

#[allow(clippy::needless_pass_by_value)]
fn consumer_error(error: sqlx::Error) -> EventError {
    EventError::Consumer(error.to_string())
}

#[derive(Debug, Serialize)]
struct DeliveryView {
    event_id: Uuid,
    kind: String,
    recipient: String,
    subject: String,
    status: String,
    attempts: i32,
    last_error: Option<String>,
    created_at: DateTime<Utc>,
    last_attempted_at: Option<DateTime<Utc>>,
    sent_at: Option<DateTime<Utc>>,
}

async fn list_deliveries(
    State(state): State<NotificationsCapability>,
    Extension(principal): Extension<AdminPrincipal>,
) -> Result<Json<Value>, NotificationsError> {
    if !principal.allows("notifications:read") {
        return Err(NotificationsError::Forbidden);
    }
    let rows = sqlx::query(
        "SELECT event_id, kind, recipient, subject, status, attempts,
                last_error, created_at, last_attempted_at, sent_at
         FROM notifications.deliveries
         ORDER BY created_at DESC
         LIMIT 250",
    )
    .fetch_all(state.database()?.pool())
    .await
    .map_err(unavailable)?;
    let deliveries = rows
        .iter()
        .map(delivery_from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(json!({ "data": deliveries })))
}

fn delivery_from_row(row: &sqlx::postgres::PgRow) -> Result<DeliveryView, NotificationsError> {
    Ok(DeliveryView {
        event_id: row.try_get("event_id").map_err(unavailable)?,
        kind: row.try_get("kind").map_err(unavailable)?,
        recipient: row.try_get("recipient").map_err(unavailable)?,
        subject: row.try_get("subject").map_err(unavailable)?,
        status: row.try_get("status").map_err(unavailable)?,
        attempts: row.try_get("attempts").map_err(unavailable)?,
        last_error: row.try_get("last_error").map_err(unavailable)?,
        created_at: row.try_get("created_at").map_err(unavailable)?,
        last_attempted_at: row.try_get("last_attempted_at").map_err(unavailable)?,
        sent_at: row.try_get("sent_at").map_err(unavailable)?,
    })
}

#[allow(clippy::needless_pass_by_value)]
fn unavailable(error: impl ToString) -> NotificationsError {
    NotificationsError::Unavailable(error.to_string())
}

#[derive(Debug, Error)]
enum NotificationsError {
    #[error("missing required scope `notifications:read`")]
    Forbidden,
    #[error("notifications service unavailable: {0}")]
    Unavailable(String),
}

impl axum::response::IntoResponse for NotificationsError {
    fn into_response(self) -> axum::response::Response {
        let (status, code) = match self {
            Self::Forbidden => (axum::http::StatusCode::FORBIDDEN, "notifications.forbidden"),
            Self::Unavailable(_) => (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                "notifications.unavailable",
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
    fn renders_verification_without_storing_body() {
        let event = EventEnvelope {
            id: Uuid::nil(),
            topic: VERIFICATION_TOPIC.into(),
            occurred_at: Utc::now(),
            correlation_id: None,
            actor_id: None,
            payload: json!({
                "email": "person@example.com",
                "token": "secret-token"
            }),
        };
        let rendered =
            render_email(&event, "no-reply@example.com", "https://app.example.com").unwrap();
        assert_eq!(rendered.kind, "email_verification");
        assert!(rendered.email.text.contains("secret-token"));
        assert_eq!(rendered.email.idempotency_key, Uuid::nil().to_string());
    }
}
