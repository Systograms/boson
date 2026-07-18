use std::{
    fs::{self, File},
    io::ErrorKind,
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    path::Path,
    process::Stdio,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use fs2::FileExt;
use tokio::time::{sleep, timeout};

use crate::{
    process::{
        ManagedProcess, build_packages, ensure_process_started, executable_path, run_migrations,
        spawn_service,
    },
    project::Project,
    state::{LifecycleState, StateStore, UnitKind, UnitState, process_is_alive, read_last_lines},
};

const SERVER_READY_TIMEOUT: Duration = Duration::from_secs(60);
const SHUTDOWN_GRACE: Duration = Duration::from_secs(10);

#[derive(Debug, Clone)]
pub struct StatusEntry {
    pub name: String,
    pub state: String,
    pub port: Option<u16>,
    pub health: String,
    pub version: Option<String>,
}

pub struct LifecycleManager {
    project: Project,
    state_store: StateStore,
}

impl LifecycleManager {
    #[must_use]
    pub fn new(project: Project) -> Self {
        let state_store = StateStore::new(project.runtime_dir(), project.logs_dir());
        Self {
            project,
            state_store,
        }
    }

    /// Starts the complete local stack and remains attached until shutdown.
    ///
    /// # Errors
    ///
    /// Returns an actionable error when validation, startup, migration, health
    /// gating, or supervision fails.
    #[allow(clippy::too_many_lines)]
    pub async fn start(&self) -> Result<()> {
        self.state_store.initialize()?;
        let lock_file = File::options()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.state_store.lock_path())
            .context("open Boson lifecycle lock")?;
        if let Err(error) = lock_file.try_lock_exclusive() {
            if error.kind() == ErrorKind::WouldBlock {
                bail!(
                    "Boson is already running for this project\nfix: use `boson status` or `boson logs`"
                );
            }
            return Err(error).context("acquire Boson lifecycle lock");
        }

        self.reconcile_stale_state()?;
        self.state_store
            .reset_logs(&["server", "worker", "storage", "mail"])?;
        self.validate().await?;

        let mut state = LifecycleState::new(
            self.project.manifest.name.clone(),
            self.project.config.snapshot_id(),
        );
        self.prepare_local_resources(&mut state)?;

        let packages = [
            self.project.manifest.migrate_package.as_str(),
            self.project.manifest.server_package.as_str(),
            self.project.manifest.worker_package.as_str(),
        ];
        let target_directory = build_packages(&self.project.root, &packages).await?;
        let migrate_executable =
            executable_path(&target_directory, &self.project.manifest.migrate_package);
        run_migrations(
            &migrate_executable,
            &self.project.root,
            &self.project.config_path,
        )
        .await?;

        let server_log = self.state_store.log_path("server");
        let worker_log = self.state_store.log_path("worker");
        let server_executable =
            executable_path(&target_directory, &self.project.manifest.server_package);
        let worker_executable =
            executable_path(&target_directory, &self.project.manifest.worker_package);
        let mut server = spawn_service(
            "server",
            &server_executable,
            &self.project.root,
            &self.project.config_path,
            &server_log,
        )
        .await?;
        let mut worker = match spawn_service(
            "worker",
            &worker_executable,
            &self.project.root,
            &self.project.config_path,
            &worker_log,
        )
        .await
        {
            Ok(process) => process,
            Err(error) => {
                server.stop(SHUTDOWN_GRACE).await;
                return Err(error);
            }
        };

        state.units.extend([
            UnitState {
                name: "server".into(),
                kind: UnitKind::Process,
                pid: Some(server.pid),
                port: Some(self.project.config.http.port),
                version: Some(env!("CARGO_PKG_VERSION").into()),
                log_path: server_log,
            },
            UnitState {
                name: "worker".into(),
                kind: UnitKind::Process,
                pid: Some(worker.pid),
                port: None,
                version: Some(env!("CARGO_PKG_VERSION").into()),
                log_path: worker_log,
            },
        ]);
        self.state_store.save(&state)?;

        let startup = async {
            ensure_process_started(&mut server, Duration::from_millis(750)).await?;
            ensure_process_started(&mut worker, Duration::from_millis(750)).await?;
            self.wait_for_server().await?;
            Result::<()>::Ok(())
        }
        .await;
        if let Err(error) = startup {
            server.stop(SHUTDOWN_GRACE).await;
            worker.stop(SHUTDOWN_GRACE).await;
            self.state_store.clear()?;
            return Err(error);
        }

        println!("[server] {}", self.project.server_url());
        println!("[worker] started");
        println!("[boson] running · press Ctrl+C to stop");

        let supervision = self.supervise(&mut server, &mut worker).await;
        println!("[boson] shutting down");
        server.stop(SHUTDOWN_GRACE).await;
        worker.stop(SHUTDOWN_GRACE).await;
        self.state_store.clear()?;
        fs2::FileExt::unlock(&lock_file).ok();
        println!("[boson] stopped");
        supervision
    }

    async fn supervise(
        &self,
        server: &mut ManagedProcess,
        worker: &mut ManagedProcess,
    ) -> Result<()> {
        loop {
            tokio::select! {
                signal = shutdown_signal() => {
                    signal?;
                    return Ok(());
                }
                () = sleep(Duration::from_secs(1)) => {
                    if let Some(status) = server.try_wait()? {
                        bail!("server exited unexpectedly with {status}\nfix: run `boson logs server`");
                    }
                    if let Some(status) = worker.try_wait()? {
                        bail!("worker exited unexpectedly with {status}\nfix: run `boson logs worker`");
                    }
                }
            }
        }
    }

    async fn validate(&self) -> Result<()> {
        let cargo = tokio::process::Command::new("cargo")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .with_context(|| "Rust is not installed\nfix: install Rust from https://rustup.rs")?;
        if !cargo.success() {
            bail!("Rust toolchain is unavailable\nfix: run `rustup update`");
        }
        ensure_port_available(self.project.config.http.port, "server")?;
        Ok(())
    }

    fn prepare_local_resources(&self, state: &mut LifecycleState) -> Result<()> {
        for (name, path) in [
            (
                "storage",
                self.project
                    .root
                    .join(&self.project.config.storage.local_root),
            ),
            (
                "mail",
                self.project.root.join(&self.project.config.mail.local_root),
            ),
        ] {
            fs::create_dir_all(&path)
                .with_context(|| format!("create local {name} directory {}", path.display()))?;
            println!("[{name}] ready at {}", path.display());
            state.units.push(UnitState {
                name: name.into(),
                kind: UnitKind::LocalResource,
                pid: None,
                port: None,
                version: None,
                log_path: self.state_store.log_path(name),
            });
        }
        Ok(())
    }

    async fn wait_for_server(&self) -> Result<()> {
        let url = format!("{}/readyz", self.project.server_url());
        wait_for_http(&url, SERVER_READY_TIMEOUT, "server").await
    }

    fn reconcile_stale_state(&self) -> Result<()> {
        if let Some(state) = self.state_store.load()? {
            if process_is_alive(state.supervisor_pid) {
                bail!(
                    "Boson is already running for this project\nfix: use `boson status` or `boson logs`"
                );
            }
            self.state_store.clear()?;
        }
        Ok(())
    }

    /// Requests graceful shutdown from the active foreground supervisor.
    ///
    /// # Errors
    ///
    /// Returns an error when no stack is running or the supervisor cannot be signalled.
    pub async fn stop(&self) -> Result<()> {
        let Some(state) = self.state_store.load()? else {
            println!("Boson is already stopped");
            return Ok(());
        };
        if process_is_alive(state.supervisor_pid) {
            send_terminate(state.supervisor_pid)?;
            for _ in 0..100 {
                if !self.state_store.state_path().exists() {
                    println!("Boson stopped");
                    return Ok(());
                }
                sleep(Duration::from_millis(100)).await;
            }
            bail!("Boson did not stop within 10 seconds\nfix: run `boson doctor`");
        }

        for unit in state
            .units
            .iter()
            .filter(|unit| unit.kind == UnitKind::Process)
        {
            if let Some(pid) = unit.pid {
                send_terminate(pid).ok();
            }
        }
        self.state_store.clear()?;
        println!("Removed stale Boson process state");
        Ok(())
    }

    /// Returns reconciled process, port, health, and version status.
    ///
    /// # Errors
    ///
    /// Returns an error when lifecycle state is unreadable.
    pub async fn status(&self) -> Result<Vec<StatusEntry>> {
        let state = self.state_store.load()?;
        let mut entries = Vec::new();
        if let Some(state) = state {
            for unit in state.units {
                if unit.kind == UnitKind::Infrastructure {
                    continue;
                }
                let running = match unit.kind {
                    UnitKind::Process => unit.pid.is_some_and(process_is_alive),
                    UnitKind::LocalResource => true,
                    UnitKind::Infrastructure => false,
                };
                let health = if unit.name == "server" && running {
                    http_is_healthy(&format!("{}/readyz", self.project.server_url())).await
                } else if running {
                    "ready".into()
                } else {
                    "unavailable".into()
                };
                entries.push(StatusEntry {
                    name: unit.name,
                    state: if running { "running" } else { "stopped" }.into(),
                    port: unit.port,
                    health,
                    version: unit.version,
                });
            }
        } else {
            for name in ["storage", "mail", "server", "worker"] {
                entries.push(StatusEntry {
                    name: name.into(),
                    state: "stopped".into(),
                    port: match name {
                        "server" => Some(self.project.config.http.port),
                        _ => None,
                    },
                    health: "unavailable".into(),
                    version: None,
                });
            }
        }
        Ok(entries)
    }

    /// Prints recent logs and optionally follows updates.
    ///
    /// # Errors
    ///
    /// Returns an error when a requested unit is unknown or log files cannot be read.
    pub async fn logs(&self, unit: Option<&str>, follow: bool, lines: usize) -> Result<()> {
        self.state_store.initialize()?;
        let names = if let Some(unit) = unit {
            vec![unit.to_owned()]
        } else {
            vec!["server".into(), "worker".into()]
        };
        let mut positions = std::collections::BTreeMap::new();
        for name in &names {
            let path = self.state_store.log_path(name);
            for line in read_last_lines(&path, lines)? {
                println!("[{name}] {line}");
            }
            positions.insert(name.clone(), file_len(&path));
        }
        if !follow {
            return Ok(());
        }
        loop {
            sleep(Duration::from_millis(300)).await;
            for name in &names {
                let path = self.state_store.log_path(name);
                let position = positions.entry(name.clone()).or_default();
                let contents = fs::read_to_string(&path).unwrap_or_default();
                if contents.len() > *position {
                    if let Some(new) = contents.get(*position..) {
                        for line in new.lines() {
                            println!("[{name}] {line}");
                        }
                    }
                    *position = contents.len();
                }
            }
        }
    }
}

async fn wait_for_http(url: &str, wait: Duration, name: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let started = std::time::Instant::now();
    while started.elapsed() < wait {
        if let Ok(response) = client.get(url).send().await
            && response.status().is_success()
        {
            return Ok(());
        }
        sleep(Duration::from_millis(500)).await;
    }
    bail!("{name} did not become healthy at {url}\nfix: run `boson logs {name}`")
}

async fn http_is_healthy(url: &str) -> String {
    match timeout(Duration::from_secs(2), reqwest::get(url)).await {
        Ok(Ok(response)) if response.status().is_success() => "healthy".into(),
        Ok(Ok(response)) => format!("HTTP {}", response.status()),
        _ => "unreachable".into(),
    }
}

fn file_len(path: &Path) -> usize {
    fs::metadata(path)
        .ok()
        .and_then(|metadata| usize::try_from(metadata.len()).ok())
        .unwrap_or(0)
}

fn ensure_port_available(port: u16, service: &str) -> Result<()> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    if TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_ok()
        || TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).is_err()
    {
        bail!(
            "{service} port {port} is already in use\nfix: run `boson status`; if Boson is stopped, close the process using port {port}"
        );
    }
    Ok(())
}

#[cfg(unix)]
fn send_terminate(pid: u32) -> Result<()> {
    let raw_pid = i32::try_from(pid).context("invalid process id")?;
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(raw_pid),
        nix::sys::signal::Signal::SIGTERM,
    )
    .context("signal Boson supervisor")
}

#[cfg(not(unix))]
fn send_terminate(_pid: u32) -> Result<()> {
    bail!("graceful stop is not yet supported on this platform")
}

async fn shutdown_signal() -> Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .context("listen for terminate signal")?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result.context("listen for Ctrl+C"),
            _ = terminate.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await.context("listen for Ctrl+C")
    }
}

#[cfg(test)]
mod tests {
    use boson_kernel::PlatformConfig;
    use tempfile::tempdir;

    use super::*;
    use crate::project::ProjectManifest;

    #[tokio::test]
    async fn stopped_project_reports_expected_units_without_external_tools() {
        let dir = tempdir().unwrap();
        let project = Project {
            root: dir.path().to_path_buf(),
            manifest: ProjectManifest::for_name("demo"),
            config_path: dir.path().join(".boson/config.yaml"),
            config: PlatformConfig::default(),
        };
        let entries = LifecycleManager::new(project).status().await.unwrap();
        assert!(entries.iter().all(|entry| entry.state == "stopped"));
        assert!(entries.iter().any(|entry| entry.name == "server"));
        assert!(!entries.iter().any(|entry| entry.name == "postgres"));
        assert!(!entries.iter().any(|entry| entry.name == "dashboard"));
    }
}
