//! Stable platform primitives shared by Boson applications and capabilities.

use std::{
    collections::{BTreeMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    path::Path,
};

use chrono::{DateTime, Utc};
use config::{Config as ConfigLoader, Environment, File};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
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
    pub auth: AuthConfig,
    pub storage: StorageConfig,
    pub queue: QueueConfig,
    pub mail: MailConfig,
    pub database_inspection: DatabaseInspectionConfig,
    /// Typed capability-owned configuration keyed by capability name.
    ///
    /// Core sections remain closed (`deny_unknown_fields`). Capability authors
    /// deserialize their own section with [`PlatformConfig::capability_config`].
    #[serde(default)]
    pub capabilities: BTreeMap<String, serde_json::Value>,
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
        if let Some(auth) = value.get_mut("auth").and_then(|v| v.as_object_mut()) {
            auth.insert(
                "jwt_secret".into(),
                serde_json::Value::String("[REDACTED]".into()),
            );
        }
        if let Some(storage) = value.get_mut("storage").and_then(|v| v.as_object_mut()) {
            for field in ["local_root", "access_key_id", "secret_access_key"] {
                storage.insert(field.into(), serde_json::Value::String("[REDACTED]".into()));
            }
        }
        if let Some(mail) = value.get_mut("mail").and_then(|v| v.as_object_mut()) {
            for field in ["local_root", "username", "password"] {
                mail.insert(field.into(), serde_json::Value::String("[REDACTED]".into()));
            }
        }
        value
    }

    /// Deserializes the namespaced `capabilities.<name>` section.
    ///
    /// # Errors
    ///
    /// Returns [`KernelError::InvalidConfig`] when the section exists but does
    /// not match `T`.
    pub fn capability_config<T: DeserializeOwned>(
        &self,
        name: &str,
    ) -> Result<Option<T>, KernelError> {
        let Some(value) = self.capabilities.get(name) else {
            return Ok(None);
        };
        serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|error| {
                KernelError::InvalidConfig(format!("capabilities.{name} is invalid: {error}"))
            })
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
    /// Optional directory of built Dashboard assets served by the Server.
    /// Empty disables static serving. Packaging may populate this path.
    pub dashboard_dir: String,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            host: "0.0.0.0".into(),
            port: 8080,
            dashboard_dir: String::new(),
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

/// Guardrails for the privileged, read-only database explorer.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DatabaseInspectionConfig {
    /// Disabled by default; deployments must opt in explicitly.
    pub enabled: bool,
    /// Empty means every non-system namespace. Production deployments should
    /// set an explicit allowlist.
    pub allowed_namespaces: Vec<String>,
    /// Case-insensitive exact column names whose values are never queried.
    pub redacted_columns: Vec<String>,
    pub statement_timeout_ms: u64,
    pub max_page_size: u32,
}

impl Default for DatabaseInspectionConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            allowed_namespaces: Vec::new(),
            redacted_columns: vec![
                "password_hash".into(),
                "token_hash".into(),
                "refresh_token_hash".into(),
                "payload".into(),
                "jwt_secret".into(),
                "bootstrap_token".into(),
            ],
            statement_timeout_ms: 2_000,
            max_page_size: 100,
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

/// End-user authentication settings consumed by the identity capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AuthConfig {
    /// Issuer claim stamped into and required from access tokens.
    pub issuer: String,
    /// HS256 signing secret. Production should use a secret source.
    pub jwt_secret: String,
    /// Access token lifetime in seconds.
    pub access_ttl_seconds: u64,
    /// Refresh session lifetime in days.
    pub refresh_ttl_days: u64,
    /// Email verification link lifetime in hours.
    pub email_verification_ttl_hours: u64,
    /// Password reset link lifetime in minutes.
    pub password_reset_ttl_minutes: u64,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            issuer: "boson".into(),
            jwt_secret: String::new(),
            access_ttl_seconds: 900,
            refresh_ttl_days: 30,
            email_verification_ttl_hours: 24,
            password_reset_ttl_minutes: 60,
        }
    }
}

/// Object storage settings consumed by the composition roots.
///
/// Capabilities never read this directly; the Server and Worker select a
/// concrete `ObjectStore` adapter from it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StorageConfig {
    /// Object store provider: `local` or `s3`.
    pub provider: String,
    /// Root directory used by the local provider. Redacted in Admin output.
    pub local_root: String,
    /// Reserved for providers that mint browser-facing URLs.
    pub public_base_url: String,
    /// Custom S3 endpoint URL (for example MinIO). Empty uses the AWS default.
    pub endpoint: String,
    /// S3 region.
    pub region: String,
    /// S3 bucket receiving platform objects.
    pub bucket: String,
    /// S3 access key. Redacted in Admin output.
    pub access_key_id: String,
    /// S3 secret key. Redacted in Admin output.
    pub secret_access_key: String,
    /// Use path-style addressing (required by MinIO and most S3-compatibles).
    pub force_path_style: bool,
}

/// Durable background queue settings selected by each composition root.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct QueueConfig {
    pub provider: String,
    pub poll_interval_ms: u64,
    pub batch_size: usize,
    pub visibility_timeout_seconds: u64,
    pub max_attempts: u32,
}

impl Default for QueueConfig {
    fn default() -> Self {
        Self {
            provider: "postgres".into(),
            poll_interval_ms: 1_000,
            batch_size: 25,
            visibility_timeout_seconds: 60,
            max_attempts: 5,
        }
    }
}

/// Email delivery settings selected by the Server and Worker composition roots.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MailConfig {
    /// Mail provider: `local` or `smtp`.
    pub provider: String,
    /// Sender stamped on platform email.
    pub from: String,
    /// Directory where the local adapter writes JSON messages.
    pub local_root: String,
    /// Browser-facing application URL used to construct action links.
    pub public_app_url: String,
    /// SMTP relay host.
    pub host: String,
    /// SMTP relay port. 587 pairs with `starttls`, 465 with `tls`.
    pub port: u16,
    /// SMTP username. Redacted in Admin output. Empty disables authentication.
    pub username: String,
    /// SMTP password. Redacted in Admin output.
    pub password: String,
    /// Transport security: `starttls` (default), `tls`, or `none`.
    pub tls: String,
}

impl Default for MailConfig {
    fn default() -> Self {
        Self {
            provider: "local".into(),
            from: "Boson <no-reply@localhost>".into(),
            local_root: "data/mail".into(),
            public_app_url: "http://localhost:3000".into(),
            host: String::new(),
            port: 587,
            username: String::new(),
            password: String::new(),
            tls: "starttls".into(),
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            provider: "local".into(),
            local_root: "data/storage".into(),
            public_base_url: String::new(),
            endpoint: String::new(),
            region: String::new(),
            bucket: String::new(),
            access_key_id: String::new(),
            secret_access_key: String::new(),
            force_path_style: false,
        }
    }
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
        let redacted = config.redacted();
        assert_eq!(redacted["database"]["url"], "[REDACTED]");
        assert_eq!(redacted["admin"]["bootstrap_token"], "[REDACTED]");
        assert_eq!(redacted["auth"]["jwt_secret"], "[REDACTED]");
        assert_eq!(redacted["storage"]["local_root"], "[REDACTED]");
        assert_eq!(redacted["storage"]["access_key_id"], "[REDACTED]");
        assert_eq!(redacted["storage"]["secret_access_key"], "[REDACTED]");
        assert_eq!(redacted["mail"]["local_root"], "[REDACTED]");
        assert_eq!(redacted["mail"]["username"], "[REDACTED]");
        assert_eq!(redacted["mail"]["password"], "[REDACTED]");
    }

    #[test]
    fn storage_defaults_to_local_provider() {
        let storage = StorageConfig::default();
        assert_eq!(storage.provider, "local");
        assert!(!storage.local_root.is_empty());
        assert!(!storage.force_path_style);
        assert!(storage.bucket.is_empty());
    }

    #[test]
    fn mail_defaults_prefer_starttls() {
        let mail = MailConfig::default();
        assert_eq!(mail.provider, "local");
        assert_eq!(mail.port, 587);
        assert_eq!(mail.tls, "starttls");
        assert!(mail.host.is_empty());
    }

    #[test]
    fn auth_defaults_are_sensible() {
        let auth = AuthConfig::default();
        assert_eq!(auth.access_ttl_seconds, 900);
        assert_eq!(auth.refresh_ttl_days, 30);
        assert_eq!(auth.email_verification_ttl_hours, 24);
        assert_eq!(auth.password_reset_ttl_minutes, 60);
        assert_eq!(auth.issuer, "boson");
    }

    #[test]
    fn database_inspection_fails_closed_by_default() {
        let inspection = DatabaseInspectionConfig::default();
        assert!(!inspection.enabled);
        assert_eq!(inspection.max_page_size, 100);
        assert!(
            inspection
                .redacted_columns
                .iter()
                .any(|column| column == "password_hash")
        );
    }

    #[test]
    fn snapshot_is_stable_for_same_config() {
        let config = PlatformConfig::default();
        assert_eq!(config.snapshot_id(), config.snapshot_id());
    }

    #[test]
    fn capability_config_deserializes_namespaced_sections() {
        #[derive(Debug, Deserialize, PartialEq, Eq)]
        struct TodosConfig {
            max_items: u32,
        }

        let mut config = PlatformConfig::default();
        config
            .capabilities
            .insert("todos".into(), serde_json::json!({ "max_items": 25 }));
        let todos = config
            .capability_config::<TodosConfig>("todos")
            .unwrap()
            .unwrap();
        assert_eq!(todos, TodosConfig { max_items: 25 });
        assert!(
            config
                .capability_config::<TodosConfig>("missing")
                .unwrap()
                .is_none()
        );
    }
}
