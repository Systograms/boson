//! Versioned domain event contracts and consumer registration.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub id: Uuid,
    pub topic: String,
    pub occurred_at: DateTime<Utc>,
    pub correlation_id: Option<String>,
    pub actor_id: Option<Uuid>,
    pub payload: Value,
}

impl EventEnvelope {
    #[must_use]
    pub fn new(topic: impl Into<String>, payload: Value) -> Self {
        Self {
            id: Uuid::now_v7(),
            topic: topic.into(),
            occurred_at: Utc::now(),
            correlation_id: None,
            actor_id: None,
            payload,
        }
    }
}

#[derive(Debug, Error)]
pub enum EventError {
    #[error("consumer failed: {0}")]
    Consumer(String),
    #[error("event is invalid: {0}")]
    Invalid(String),
}

#[async_trait]
pub trait EventConsumer: Send + Sync {
    fn name(&self) -> &'static str;
    /// The topic this consumer subscribes to. Return `"*"` to receive every
    /// event (used by cross-cutting consumers such as the audit trail).
    fn topic(&self) -> &'static str;
    async fn handle(&self, event: &EventEnvelope) -> Result<(), EventError>;
}

pub trait EventCatalog {
    fn schemas(&self) -> &'static [&'static str];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_versioned_event() {
        let event = EventEnvelope::new("identity.user_created.v1", serde_json::json!({}));
        assert_eq!(event.topic, "identity.user_created.v1");
    }
}
