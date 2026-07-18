use std::{path::PathBuf, process::Stdio, time::Duration};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use tokio::{
    process::{Child, Command},
    time::sleep,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InfrastructureStatus {
    Running,
    Stopped,
    Unavailable(String),
}

#[async_trait]
pub trait InfrastructureBackend: Send + Sync {
    async fn validate(&self) -> Result<()>;
    async fn start(&self, services: &[String]) -> Result<()>;
    async fn wait_ready(&self, service: &str, timeout: Duration) -> Result<()>;
    async fn stop(&self, services: &[String]) -> Result<()>;
    async fn status(&self, service: &str) -> Result<InfrastructureStatus>;
    async fn spawn_logs(&self, service: &str) -> Result<Child>;
}

/// Docker Compose implementation hidden behind the orchestration backend.
///
/// Compose is an internal implementation detail. Users only interact with
/// `boson start`, `stop`, `status`, and `logs`.
#[derive(Debug, Clone)]
pub struct DockerComposeBackend {
    root: PathBuf,
    compose_path: PathBuf,
}

impl DockerComposeBackend {
    #[must_use]
    pub fn new(root: PathBuf, compose_path: PathBuf) -> Self {
        Self { root, compose_path }
    }

    fn command(&self) -> Command {
        let mut command = Command::new("docker");
        command
            .arg("compose")
            .arg("-f")
            .arg(&self.compose_path)
            .current_dir(&self.root);
        command
    }

    async fn output(&self, args: &[&str]) -> Result<std::process::Output> {
        self.command()
            .args(args)
            .output()
            .await
            .with_context(|| "Docker is not available\nfix: install and start Docker Desktop")
    }
}

#[async_trait]
impl InfrastructureBackend for DockerComposeBackend {
    async fn validate(&self) -> Result<()> {
        if !self.compose_path.is_file() {
            bail!(
                "managed infrastructure is enabled but {} is missing\nfix: restore the project compose file or disable managed infrastructure",
                self.compose_path.display()
            );
        }
        let output = Command::new("docker")
            .args(["info", "--format", "{{.ServerVersion}}"])
            .output()
            .await
            .with_context(|| "Docker is not installed\nfix: install Docker Desktop")?;
        if !output.status.success() {
            bail!(
                "Docker is installed but the daemon is not running\nfix: open Docker Desktop and wait until it reports that Docker is running"
            );
        }
        Ok(())
    }

    async fn start(&self, services: &[String]) -> Result<()> {
        let mut command = self.command();
        command.args(["up", "-d", "--build", "--remove-orphans"]);
        command.args(services);
        let status = command
            .status()
            .await
            .context("start managed infrastructure")?;
        if !status.success() {
            bail!(
                "managed infrastructure failed to start\nfix: run `boson doctor` and inspect `.boson/logs`"
            );
        }
        Ok(())
    }

    async fn wait_ready(&self, service: &str, timeout: Duration) -> Result<()> {
        let started = std::time::Instant::now();
        while started.elapsed() < timeout {
            if service == "postgres" {
                let mut command = self.command();
                let ready = command
                    .args(["exec", "-T", service, "pg_isready", "-U", "boson"])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .await
                    .is_ok_and(|status| status.success());
                if ready {
                    return Ok(());
                }
            } else if matches!(self.status(service).await?, InfrastructureStatus::Running) {
                return Ok(());
            }
            sleep(Duration::from_millis(500)).await;
        }
        bail!(
            "{service} did not become healthy within {} seconds\nfix: run `boson logs {service}`",
            timeout.as_secs()
        )
    }

    async fn stop(&self, services: &[String]) -> Result<()> {
        if services.is_empty() {
            return Ok(());
        }
        let mut command = self.command();
        command.arg("stop").args(services);
        let status = command
            .status()
            .await
            .context("stop managed infrastructure")?;
        if !status.success() {
            bail!("managed infrastructure did not stop cleanly");
        }
        Ok(())
    }

    async fn status(&self, service: &str) -> Result<InfrastructureStatus> {
        let output = self
            .output(&["ps", "--status", "running", "--services"])
            .await?;
        if !output.status.success() {
            return Ok(InfrastructureStatus::Unavailable(
                String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            ));
        }
        let running = String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|name| name.trim() == service);
        Ok(if running {
            InfrastructureStatus::Running
        } else {
            InfrastructureStatus::Stopped
        })
    }

    async fn spawn_logs(&self, service: &str) -> Result<Child> {
        self.command()
            .args(["logs", "--follow", "--tail", "50", "--no-color", service])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("stream {service} logs"))
    }
}
