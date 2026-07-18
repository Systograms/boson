//! Stable platform primitives shared by Boson applications and capabilities.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::Path,
};

use chrono::{DateTime, Utc};
use config::{Config as ConfigLoader, Environment, File};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlatformConfig {
    pub app: AppConfig,
    pub http: HttpConfig,
    pub database: DatabaseConfig,
    pub telemetry: TelemetryConfig,
    pub admin: AdminConfig,
}

impl PlatformConfig {
    /// Loads defaults, then the optional file, then `BOSON__*` environment variables.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, KernelError> {
        ConfigLoader::builder()
            .add_source(ConfigLoader::try_from(&Self::default())?)
            .add_source(File::from(path.as_ref()).required(false))
            .add_source(
                Environment::with_prefix("BOSON")
                    .prefix_separator("__")
                    .separator("__"),
            )
            .build()?
            .try_deserialize()
            .map_err(KernelError::Config)
    }

    #[must_use]
    pub fn snapshot_id(&self) -> String {
        let mut hasher = DefaultHasher::new();
        serde_json::to_string(self)
            .expect("PlatformConfig is serializable")
            .hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    /// Returns a representation safe for Admin APIs and logs.
    #[must_use]
    pub fn redacted(&self) -> serde_json::Value {
        let mut value = serde_json::to_value(self).expect("PlatformConfig is serializable");
        if let Some(database) = value.get_mut("database").and_then(|v| v.as_object_mut()) {
            database.insert("url".into(), serde_json::Value::String("[REDACTED]".into()));
        }
        if let Some(admin) = value.get_mut("admin").and_then(|v| v.as_object_mut()) {
            admin.insert(
                "bootstrap_token".into(),
                serde_json::Value::String("[REDACTED]".into()),
            );
        }
        value
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppConfig {
    pub name: String,
    pub environment: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            name: "boson".into(),
            environment: "development".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HttpConfig {
    pub host: String,
    pub port: u16,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".into(),
            port: 8080,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DatabaseConfig {
    pub url: String,
    pub max_connections: u32,
    pub connect_on_boot: bool,
    pub run_migrations: bool,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: "postgres://boson:boson@localhost:5432/boson".into(),
            max_connections: 10,
            connect_on_boot: false,
            run_migrations: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TelemetryConfig {
    pub log_level: String,
    pub json: bool,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            log_level: "info".into(),
            json: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AdminConfig {
    /// Development bootstrap credential. Production should use a secret source.
    pub bootstrap_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestContext {
    pub request_id: Uuid,
    pub trace_id: Option<String>,
    pub user_id: Option<Uuid>,
    pub organization_id: Option<Uuid>,
    pub admin_id: Option<Uuid>,
    pub started_at: DateTime<Utc>,
}

impl RequestContext {
    #[must_use]
    pub fn new() -> Self {
        Self {
            request_id: Uuid::now_v7(),
            trace_id: None,
            user_id: None,
            organization_id: None,
            admin_id: None,
            started_at: Utc::now(),
        }
    }
}

impl Default for RequestContext {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Error)]
pub enum KernelError {
    #[error("configuration error: {0}")]
    Config(#[from] config::ConfigError),
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
}

pub fn init_telemetry(config: &TelemetryConfig) -> Result<(), KernelError> {
    let filter = EnvFilter::try_new(&config.log_level)
        .map_err(|error| KernelError::InvalidConfig(error.to_string()))?;
    if config.json {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(filter)
            .try_init()
            .map_err(|error| KernelError::InvalidConfig(error.to_string()))
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .try_init()
            .map_err(|error| KernelError::InvalidConfig(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_are_redacted() {
        let config = PlatformConfig::default();
        assert_eq!(config.redacted()["database"]["url"], "[REDACTED]");
    }

    #[test]
    fn snapshot_is_stable_for_same_config() {
        let config = PlatformConfig::default();
        assert_eq!(config.snapshot_id(), config.snapshot_id());
    }
}
