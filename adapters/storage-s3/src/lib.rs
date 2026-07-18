//! S3-compatible implementation of [`boson_ports::ObjectStore`].
//!
//! Works with AWS S3 and S3-compatible services such as MinIO. Requests are
//! signed locally with SigV4 (via `rusty-s3`) and executed over HTTPS with
//! `reqwest`/rustls, keeping the adapter lightweight and Sans-IO testable:
//! configuration validation, key validation, and URL signing never touch the
//! network.
//!
//! Object keys follow the same strict validation as the local adapter so keys
//! remain portable across providers. Custom metadata is stored as
//! `x-amz-meta-*` headers.

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use boson_kernel::StorageConfig;
use boson_ports::{HealthCheck, HealthStatus, Object, ObjectMetadata, ObjectStore, PortError};
use bytes::Bytes;
use rusty_s3::{Bucket, Credentials, S3Action, UrlStyle};
use url::Url;

const MAX_KEY_CHARS: usize = 512;
const METADATA_HEADER_PREFIX: &str = "x-amz-meta-";
/// Lifetime of internally generated request signatures.
const INTERNAL_SIGNATURE_TTL: Duration = Duration::from_secs(300);

/// An [`ObjectStore`] backed by an S3-compatible bucket.
#[derive(Debug, Clone)]
pub struct S3ObjectStore {
    bucket: Bucket,
    credentials: Credentials,
    client: reqwest::Client,
}

impl S3ObjectStore {
    /// Builds the store from platform configuration without touching the
    /// network.
    ///
    /// # Errors
    ///
    /// Returns [`PortError::Invalid`] when the bucket, region, credentials,
    /// or endpoint are missing or malformed.
    pub fn from_config(config: &StorageConfig) -> Result<Self, PortError> {
        let invalid = |message: &str| PortError::Invalid(format!("storage.{message}"));
        if config.bucket.trim().is_empty() {
            return Err(invalid("bucket must not be empty for the s3 provider"));
        }
        if config.region.trim().is_empty() {
            return Err(invalid("region must not be empty for the s3 provider"));
        }
        if config.access_key_id.trim().is_empty() {
            return Err(invalid(
                "access_key_id must not be empty for the s3 provider",
            ));
        }
        if config.secret_access_key.trim().is_empty() {
            return Err(invalid(
                "secret_access_key must not be empty for the s3 provider",
            ));
        }

        let endpoint = if config.endpoint.trim().is_empty() {
            format!("https://s3.{}.amazonaws.com", config.region)
        } else {
            config.endpoint.trim().to_owned()
        };
        let endpoint: Url = endpoint
            .parse()
            .map_err(|error| invalid(&format!("endpoint is not a valid URL: {error}")))?;
        if endpoint.scheme() != "https" && endpoint.scheme() != "http" {
            return Err(invalid("endpoint must use http or https"));
        }

        let url_style = if config.force_path_style {
            UrlStyle::Path
        } else {
            UrlStyle::VirtualHost
        };
        let bucket = Bucket::new(endpoint, url_style, config.bucket.clone(), config.region.clone())
            .map_err(|error| invalid(&format!("endpoint is unusable: {error}")))?;
        let credentials = Credentials::new(
            config.access_key_id.clone(),
            config.secret_access_key.clone(),
        );
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|error| PortError::Unavailable(error.to_string()))?;
        Ok(Self {
            bucket,
            credentials,
            client,
        })
    }

    fn signed_get_url(&self, key: &str, ttl: Duration) -> Result<Url, PortError> {
        validate_key(key)?;
        Ok(self
            .bucket
            .get_object(Some(&self.credentials), key)
            .sign(ttl))
    }

    fn signed_put_url(&self, key: &str, ttl: Duration) -> Result<Url, PortError> {
        validate_key(key)?;
        Ok(self
            .bucket
            .put_object(Some(&self.credentials), key)
            .sign(ttl))
    }
}

/// Splits a logical object key into safe path components.
///
/// Mirrors the local adapter so keys stay portable between providers.
///
/// # Errors
///
/// Returns [`PortError::Invalid`] for empty keys, absolute keys, keys longer
/// than 512 characters, and keys containing empty, `.`, `..`, backslash, or
/// control-character components.
pub fn validate_key(key: &str) -> Result<Vec<&str>, PortError> {
    let invalid = |reason: &str| PortError::Invalid(format!("invalid object key: {reason}"));
    if key.is_empty() {
        return Err(invalid("key must not be empty"));
    }
    if key.chars().count() > MAX_KEY_CHARS {
        return Err(invalid("key exceeds 512 characters"));
    }
    if key.starts_with('/') {
        return Err(invalid("key must be relative, not absolute"));
    }
    key.split('/')
        .map(|component| {
            if component.is_empty() {
                return Err(invalid("key must not contain empty components"));
            }
            if component == "." || component == ".." {
                return Err(invalid("key must not contain `.` or `..` components"));
            }
            if component.contains('\\') {
                return Err(invalid("key must use `/` separators only"));
            }
            if component.chars().any(char::is_control) {
                return Err(invalid("key must not contain control characters"));
            }
            Ok(component)
        })
        .collect()
}

fn provider_error(error: &reqwest::Error) -> PortError {
    if error.is_connect() || error.is_timeout() {
        PortError::Unavailable(error.to_string())
    } else {
        PortError::Provider(error.to_string())
    }
}

fn status_error(operation: &str, status: reqwest::StatusCode) -> PortError {
    if status == reqwest::StatusCode::NOT_FOUND {
        PortError::NotFound
    } else {
        PortError::Provider(format!("s3 {operation} failed with status {status}"))
    }
}

#[async_trait]
impl HealthCheck for S3ObjectStore {
    async fn check(&self) -> HealthStatus {
        let started = Instant::now();
        let url = self
            .bucket
            .head_bucket(Some(&self.credentials))
            .sign(INTERNAL_SIGNATURE_TTL);
        let result = self.client.head(url).send().await;
        let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        let (healthy, message) = match result {
            Ok(response) if response.status().is_success() => (true, None),
            Ok(response) => (
                false,
                Some(format!("bucket check returned {}", response.status())),
            ),
            Err(error) => (false, Some(error.to_string())),
        };
        HealthStatus {
            component: "storage-s3".into(),
            healthy,
            message,
            latency_ms,
        }
    }
}

#[async_trait]
impl ObjectStore for S3ObjectStore {
    async fn put(
        &self,
        key: &str,
        bytes: Bytes,
        metadata: ObjectMetadata,
    ) -> Result<(), PortError> {
        validate_key(key)?;
        let mut action = self.bucket.put_object(Some(&self.credentials), key);
        let mut headers = Vec::new();
        if let Some(content_type) = &metadata.content_type {
            headers.push(("content-type".to_owned(), content_type.clone()));
        }
        for (name, value) in &metadata.custom {
            headers.push((format!("{METADATA_HEADER_PREFIX}{name}"), value.clone()));
        }
        for (name, value) in &headers {
            action.headers_mut().insert(name.as_str(), value.as_str());
        }
        let url = action.sign(INTERNAL_SIGNATURE_TTL);

        let mut request = self.client.put(url).body(bytes);
        for (name, value) in &headers {
            request = request.header(name, value);
        }
        let response = request.send().await.map_err(|error| provider_error(&error))?;
        if !response.status().is_success() {
            return Err(status_error("put", response.status()));
        }
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Object, PortError> {
        let url = self.signed_get_url(key, INTERNAL_SIGNATURE_TTL)?;
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|error| provider_error(&error))?;
        if !response.status().is_success() {
            return Err(status_error("get", response.status()));
        }
        let mut custom = BTreeMap::new();
        let mut content_type = None;
        for (name, value) in response.headers() {
            let Ok(value) = value.to_str() else { continue };
            let name = name.as_str();
            if name.eq_ignore_ascii_case("content-type") {
                content_type = Some(value.to_owned());
            } else if let Some(suffix) = name
                .to_ascii_lowercase()
                .strip_prefix(METADATA_HEADER_PREFIX)
            {
                custom.insert(suffix.to_owned(), value.to_owned());
            }
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| provider_error(&error))?;
        Ok(Object {
            bytes,
            metadata: ObjectMetadata {
                content_type,
                custom,
            },
        })
    }

    async fn delete(&self, key: &str) -> Result<(), PortError> {
        validate_key(key)?;
        let url = self
            .bucket
            .delete_object(Some(&self.credentials), key)
            .sign(INTERNAL_SIGNATURE_TTL);
        let response = self
            .client
            .delete(url)
            .send()
            .await
            .map_err(|error| provider_error(&error))?;
        // S3 DELETE is idempotent and returns 204 whether or not the object
        // existed, so a missing object cannot be distinguished here.
        if !response.status().is_success() {
            return Err(status_error("delete", response.status()));
        }
        Ok(())
    }

    async fn signed_upload_url(&self, key: &str, ttl: Duration) -> Result<String, PortError> {
        Ok(self.signed_put_url(key, ttl)?.into())
    }

    async fn signed_download_url(&self, key: &str, ttl: Duration) -> Result<String, PortError> {
        Ok(self.signed_get_url(key, ttl)?.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s3_config() -> StorageConfig {
        StorageConfig {
            provider: "s3".into(),
            endpoint: "http://localhost:9000".into(),
            region: "us-east-1".into(),
            bucket: "boson".into(),
            access_key_id: "minioadmin".into(),
            secret_access_key: "minioadmin".into(),
            force_path_style: true,
            ..StorageConfig::default()
        }
    }

    #[test]
    fn traversal_and_malformed_keys_are_rejected() {
        for key in [
            "",
            "/etc/passwd",
            "../secret",
            "..",
            ".",
            "a/../b",
            "a/..",
            "a/./b",
            "a//b",
            "a/",
            "windows\\style",
            "a/\u{0}",
        ] {
            assert!(
                matches!(validate_key(key), Err(PortError::Invalid(_))),
                "accepted `{key}`"
            );
        }
    }

    #[test]
    fn valid_keys_are_accepted() {
        assert_eq!(
            validate_key("users/42/report.pdf").unwrap(),
            vec!["users", "42", "report.pdf"]
        );
        assert_eq!(validate_key("solo.txt").unwrap(), vec!["solo.txt"]);
    }

    #[test]
    fn oversized_keys_are_rejected() {
        let key = "a".repeat(MAX_KEY_CHARS + 1);
        assert!(matches!(validate_key(&key), Err(PortError::Invalid(_))));
    }

    #[test]
    fn valid_config_constructs_a_store() {
        assert!(S3ObjectStore::from_config(&s3_config()).is_ok());
    }

    #[test]
    fn empty_endpoint_defaults_to_aws() {
        let config = StorageConfig {
            endpoint: String::new(),
            force_path_style: false,
            ..s3_config()
        };
        let store = S3ObjectStore::from_config(&config).unwrap();
        let url = store
            .signed_get_url("k.txt", Duration::from_secs(60))
            .unwrap();
        assert_eq!(url.host_str(), Some("boson.s3.us-east-1.amazonaws.com"));
    }

    #[test]
    fn missing_required_fields_are_rejected() {
        for mutate in [
            |c: &mut StorageConfig| c.bucket.clear(),
            |c: &mut StorageConfig| c.region.clear(),
            |c: &mut StorageConfig| c.access_key_id.clear(),
            |c: &mut StorageConfig| c.secret_access_key.clear(),
        ] {
            let mut config = s3_config();
            mutate(&mut config);
            assert!(matches!(
                S3ObjectStore::from_config(&config),
                Err(PortError::Invalid(_))
            ));
        }
    }

    #[test]
    fn malformed_endpoints_are_rejected() {
        for endpoint in ["not a url", "ftp://example.com"] {
            let config = StorageConfig {
                endpoint: endpoint.into(),
                ..s3_config()
            };
            assert!(
                matches!(
                    S3ObjectStore::from_config(&config),
                    Err(PortError::Invalid(_))
                ),
                "accepted `{endpoint}`"
            );
        }
    }

    #[test]
    fn signed_urls_are_deterministic_in_shape() {
        let store = S3ObjectStore::from_config(&s3_config()).unwrap();
        let url = store
            .signed_get_url("users/1/file.txt", Duration::from_secs(60))
            .unwrap();
        assert_eq!(url.host_str(), Some("localhost"));
        assert_eq!(url.path(), "/boson/users/1/file.txt");
        assert!(
            url.query()
                .is_some_and(|query| query.contains("X-Amz-Signature"))
        );
    }

    #[test]
    fn signed_urls_reject_invalid_keys() {
        let store = S3ObjectStore::from_config(&s3_config()).unwrap();
        assert!(matches!(
            store.signed_get_url("../escape", Duration::from_secs(60)),
            Err(PortError::Invalid(_))
        ));
        assert!(matches!(
            store.signed_put_url("/abs", Duration::from_secs(60)),
            Err(PortError::Invalid(_))
        ));
    }
}
