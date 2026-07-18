//! Development mailer that persists messages as JSON files.
//!
//! The adapter is deliberately idempotent: the email idempotency key hashes to
//! a stable filename and an existing file is treated as an already-sent email.

use std::{path::PathBuf, time::Instant};

use async_trait::async_trait;
use boson_ports::{Email, HealthCheck, HealthStatus, Mailer, PortError};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone)]
pub struct LocalMailer {
    root: PathBuf,
}

impl LocalMailer {
    /// Opens the local mailbox, creating it when necessary.
    ///
    /// # Errors
    ///
    /// Returns an error when the path is empty or cannot be created.
    pub async fn open(root: impl Into<PathBuf>) -> Result<Self, PortError> {
        let root = root.into();
        if root.as_os_str().is_empty() {
            return Err(PortError::Invalid(
                "mail.local_root must not be empty".into(),
            ));
        }
        tokio::fs::create_dir_all(&root)
            .await
            .map_err(|error| PortError::Unavailable(error.to_string()))?;
        Ok(Self { root })
    }

    fn path_for(&self, idempotency_key: &str) -> PathBuf {
        let hash = hex::encode(Sha256::digest(idempotency_key.as_bytes()));
        self.root.join(format!("{hash}.json"))
    }
}

#[async_trait]
impl Mailer for LocalMailer {
    async fn send(&self, email: Email) -> Result<(), PortError> {
        if email.to.trim().is_empty()
            || email.from.trim().is_empty()
            || email.subject.trim().is_empty()
            || email.idempotency_key.trim().is_empty()
        {
            return Err(PortError::Invalid(
                "email recipient, sender, subject, and idempotency key are required".into(),
            ));
        }
        let path = self.path_for(&email.idempotency_key);
        let bytes = serde_json::to_vec_pretty(&email)
            .map_err(|error| PortError::Invalid(error.to_string()))?;
        let file = tokio::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .await;
        let mut file = match file {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => return Ok(()),
            Err(error) => return Err(PortError::Unavailable(error.to_string())),
        };
        file.write_all(&bytes)
            .await
            .map_err(|error| PortError::Unavailable(error.to_string()))?;
        file.sync_all()
            .await
            .map_err(|error| PortError::Unavailable(error.to_string()))
    }
}

#[async_trait]
impl HealthCheck for LocalMailer {
    async fn check(&self) -> HealthStatus {
        let started = Instant::now();
        let result = tokio::fs::metadata(&self.root).await;
        HealthStatus {
            component: "mailer.local".into(),
            healthy: result.as_ref().is_ok_and(std::fs::Metadata::is_dir),
            message: result.err().map(|error| error.to_string()),
            latency_ms: started.elapsed().as_millis().try_into().unwrap_or(u64::MAX),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn send_is_idempotent() {
        let directory = tempfile::tempdir().unwrap();
        let mailer = LocalMailer::open(directory.path()).await.unwrap();
        let email = Email {
            to: "person@example.com".into(),
            from: "Boson <no-reply@example.com>".into(),
            subject: "Welcome".into(),
            text: "Hello".into(),
            idempotency_key: "event-1".into(),
        };
        mailer.send(email.clone()).await.unwrap();
        mailer.send(email).await.unwrap();
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}
