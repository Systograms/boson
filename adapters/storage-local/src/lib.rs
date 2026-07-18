//! Local-filesystem implementation of [`boson_ports::ObjectStore`].
//!
//! Object keys are logical `/`-separated paths, never filesystem paths. Keys
//! are strictly validated so an object can never escape the configured root.
//! Object bytes live under `<root>/objects/<key>` and metadata is persisted
//! as a sidecar JSON document under `<root>/meta/<key>.json`.
//!
//! Signed URLs are not natively supported by a local filesystem. Both signed
//! URL methods return [`PortError::Invalid`]; callers such as the Files
//! capability use direct upload and download endpoints instead.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use boson_ports::{HealthCheck, HealthStatus, Object, ObjectMetadata, ObjectStore, PortError};
use bytes::Bytes;
use serde::{Deserialize, Serialize};

const MAX_KEY_CHARS: usize = 512;
const OBJECTS_DIR: &str = "objects";
const META_DIR: &str = "meta";

/// Serialized sidecar document for object metadata.
#[derive(Debug, Default, Serialize, Deserialize)]
struct StoredMetadata {
    content_type: Option<String>,
    #[serde(default)]
    custom: BTreeMap<String, String>,
}

/// An [`ObjectStore`] backed by a directory on the local filesystem.
#[derive(Debug, Clone)]
pub struct LocalObjectStore {
    root: PathBuf,
}

impl LocalObjectStore {
    /// Opens the store, creating the root directory when it does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`PortError::Invalid`] for an empty root and
    /// [`PortError::Unavailable`] when the root cannot be created.
    pub async fn open(root: impl Into<PathBuf>) -> Result<Self, PortError> {
        let root = root.into();
        if root.as_os_str().is_empty() {
            return Err(PortError::Invalid(
                "storage.local_root must not be empty".into(),
            ));
        }
        tokio::fs::create_dir_all(&root)
            .await
            .map_err(|error| PortError::Unavailable(error.to_string()))?;
        Ok(Self { root })
    }

    /// Resolves a validated logical key below the given subtree.
    fn resolve(&self, subtree: &str, key: &str) -> Result<PathBuf, PortError> {
        let mut path = self.root.join(subtree);
        for component in validate_key(key)? {
            path.push(component);
        }
        Ok(path)
    }

    fn object_path(&self, key: &str) -> Result<PathBuf, PortError> {
        self.resolve(OBJECTS_DIR, key)
    }

    fn metadata_path(&self, key: &str) -> Result<PathBuf, PortError> {
        self.resolve(META_DIR, key).map(|mut path| {
            path.as_mut_os_string().push(".json");
            path
        })
    }
}

/// Splits a logical object key into safe path components.
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

#[async_trait]
impl HealthCheck for LocalObjectStore {
    async fn check(&self) -> HealthStatus {
        let started = Instant::now();
        let result = tokio::fs::create_dir_all(&self.root).await;
        let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
        match result {
            Ok(()) => HealthStatus {
                component: "storage-local".into(),
                healthy: true,
                message: None,
                latency_ms,
            },
            Err(error) => HealthStatus {
                component: "storage-local".into(),
                healthy: false,
                message: Some(error.to_string()),
                latency_ms,
            },
        }
    }
}

#[async_trait]
impl ObjectStore for LocalObjectStore {
    async fn put(
        &self,
        key: &str,
        bytes: Bytes,
        metadata: ObjectMetadata,
    ) -> Result<(), PortError> {
        let object_path = self.object_path(key)?;
        let metadata_path = self.metadata_path(key)?;
        create_parent(&object_path).await?;
        create_parent(&metadata_path).await?;

        tokio::fs::write(&object_path, &bytes)
            .await
            .map_err(|error| PortError::Provider(error.to_string()))?;
        let sidecar = serde_json::to_vec(&StoredMetadata {
            content_type: metadata.content_type,
            custom: metadata.custom,
        })
        .map_err(|error| PortError::Provider(error.to_string()))?;
        tokio::fs::write(&metadata_path, sidecar)
            .await
            .map_err(|error| PortError::Provider(error.to_string()))?;
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Object, PortError> {
        let object_path = self.object_path(key)?;
        let bytes = match tokio::fs::read(&object_path).await {
            Ok(bytes) => Bytes::from(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(PortError::NotFound);
            }
            Err(error) => return Err(PortError::Provider(error.to_string())),
        };
        let stored = match tokio::fs::read(self.metadata_path(key)?).await {
            Ok(raw) => serde_json::from_slice::<StoredMetadata>(&raw).unwrap_or_default(),
            Err(_) => StoredMetadata::default(),
        };
        Ok(Object {
            bytes,
            metadata: ObjectMetadata {
                content_type: stored.content_type,
                custom: stored.custom,
            },
        })
    }

    async fn delete(&self, key: &str) -> Result<(), PortError> {
        let object_path = self.object_path(key)?;
        match tokio::fs::remove_file(&object_path).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(PortError::NotFound);
            }
            Err(error) => return Err(PortError::Provider(error.to_string())),
        }
        // The sidecar is advisory; a missing one must not fail the delete.
        let _ = tokio::fs::remove_file(self.metadata_path(key)?).await;
        Ok(())
    }

    async fn signed_upload_url(&self, key: &str, _ttl: Duration) -> Result<String, PortError> {
        validate_key(key)?;
        Err(PortError::Invalid(
            "local storage does not issue signed upload URLs; upload through the Files API".into(),
        ))
    }

    async fn signed_download_url(&self, key: &str, _ttl: Duration) -> Result<String, PortError> {
        validate_key(key)?;
        Err(PortError::Invalid(
            "local storage does not issue signed download URLs; download through the Files API"
                .into(),
        ))
    }
}

async fn create_parent(path: &Path) -> Result<(), PortError> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| PortError::Unavailable(error.to_string()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn store() -> (tempfile::TempDir, LocalObjectStore) {
        let dir = tempfile::tempdir().expect("create temp dir");
        let store = LocalObjectStore::open(dir.path().join("storage"))
            .await
            .expect("open store");
        (dir, store)
    }

    fn metadata(content_type: &str) -> ObjectMetadata {
        ObjectMetadata {
            content_type: Some(content_type.into()),
            custom: BTreeMap::from([("owner".into(), "tests".into())]),
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

    #[tokio::test]
    async fn put_get_roundtrip_preserves_bytes_and_metadata() {
        let (_dir, store) = store().await;
        store
            .put(
                "users/1/hello.txt",
                Bytes::from_static(b"hello world"),
                metadata("text/plain"),
            )
            .await
            .unwrap();
        let object = store.get("users/1/hello.txt").await.unwrap();
        assert_eq!(object.bytes.as_ref(), b"hello world");
        assert_eq!(object.metadata.content_type.as_deref(), Some("text/plain"));
        assert_eq!(
            object.metadata.custom.get("owner").map(String::as_str),
            Some("tests")
        );
    }

    #[tokio::test]
    async fn put_overwrites_existing_object() {
        let (_dir, store) = store().await;
        store
            .put("k", Bytes::from_static(b"one"), metadata("text/plain"))
            .await
            .unwrap();
        store
            .put(
                "k",
                Bytes::from_static(b"two"),
                metadata("application/json"),
            )
            .await
            .unwrap();
        let object = store.get("k").await.unwrap();
        assert_eq!(object.bytes.as_ref(), b"two");
        assert_eq!(
            object.metadata.content_type.as_deref(),
            Some("application/json")
        );
    }

    #[tokio::test]
    async fn delete_removes_object_and_missing_objects_are_not_found() {
        let (_dir, store) = store().await;
        store
            .put("gone.bin", Bytes::from_static(b"x"), metadata("bin"))
            .await
            .unwrap();
        store.delete("gone.bin").await.unwrap();
        assert!(matches!(
            store.get("gone.bin").await,
            Err(PortError::NotFound)
        ));
        assert!(matches!(
            store.delete("gone.bin").await,
            Err(PortError::NotFound)
        ));
        assert!(matches!(
            store.get("never-existed").await,
            Err(PortError::NotFound)
        ));
    }

    #[tokio::test]
    async fn operations_reject_traversal_keys() {
        let (_dir, store) = store().await;
        assert!(matches!(
            store
                .put("../escape", Bytes::from_static(b"x"), metadata("bin"))
                .await,
            Err(PortError::Invalid(_))
        ));
        assert!(matches!(
            store.get("../escape").await,
            Err(PortError::Invalid(_))
        ));
        assert!(matches!(
            store.delete("../escape").await,
            Err(PortError::Invalid(_))
        ));
    }

    #[tokio::test]
    async fn signed_urls_return_typed_invalid_errors() {
        let (_dir, store) = store().await;
        assert!(matches!(
            store.signed_upload_url("k", Duration::from_secs(60)).await,
            Err(PortError::Invalid(_))
        ));
        assert!(matches!(
            store
                .signed_download_url("k", Duration::from_secs(60))
                .await,
            Err(PortError::Invalid(_))
        ));
    }

    #[tokio::test]
    async fn health_check_reports_healthy_root_with_latency() {
        let (_dir, store) = store().await;
        let status = store.check().await;
        assert!(status.healthy);
        assert_eq!(status.component, "storage-local");
    }

    #[tokio::test]
    async fn open_creates_missing_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("nested/deeply/storage");
        LocalObjectStore::open(&root).await.unwrap();
        assert!(root.is_dir());
    }
}
