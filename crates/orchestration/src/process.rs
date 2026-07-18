use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
    task::JoinHandle,
    time::{sleep, timeout},
};

#[derive(Debug)]
pub struct ManagedProcess {
    pub name: String,
    pub pid: u32,
    child: Child,
    log_tasks: Vec<JoinHandle<()>>,
}

impl ManagedProcess {
    /// Sends a graceful signal, waits, then force-kills after the timeout.
    pub async fn stop(mut self, grace: Duration) {
        send_terminate(self.pid);
        if timeout(grace, self.child.wait()).await.is_err() {
            let _ = self.child.start_kill();
            let _ = self.child.wait().await;
        }
        for task in self.log_tasks.drain(..) {
            task.abort();
        }
    }

    /// Returns an exit status if the process has already exited.
    ///
    /// # Errors
    ///
    /// Returns an error when process status cannot be queried.
    pub fn try_wait(&mut self) -> Result<Option<std::process::ExitStatus>> {
        self.child.try_wait().context("query child process")
    }
}

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    target_directory: PathBuf,
}

/// Builds project service packages once and returns the target directory.
///
/// # Errors
///
/// Returns an actionable error when Cargo is unavailable or compilation fails.
pub async fn build_packages(root: &Path, packages: &[&str]) -> Result<PathBuf> {
    println!("[build] compiling project services");
    let mut command = Command::new("cargo");
    command.arg("build").current_dir(root);
    for package in packages {
        command.args(["-p", package]);
    }
    let status = command.status().await.with_context(
        || "Rust toolchain is unavailable\nfix: install Rust from https://rustup.rs",
    )?;
    if !status.success() {
        bail!(
            "project build failed\nfix: resolve the compiler errors above, then run `boson start`"
        );
    }

    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(root)
        .output()
        .await
        .context("locate Cargo target directory")?;
    if !output.status.success() {
        bail!("could not locate compiled project binaries");
    }
    let metadata: CargoMetadata =
        serde_json::from_slice(&output.stdout).context("parse Cargo metadata")?;
    Ok(metadata.target_directory)
}

#[must_use]
pub fn executable_path(target_directory: &Path, package: &str) -> PathBuf {
    let executable = format!("{package}{}", std::env::consts::EXE_SUFFIX);
    target_directory.join("debug").join(executable)
}

/// Runs the migration binary and waits for completion.
///
/// # Errors
///
/// Returns an error when migrations cannot start or do not complete successfully.
pub async fn run_migrations(executable: &Path, root: &Path, config_path: &Path) -> Result<()> {
    println!("[migrate] checking database migrations");
    let status = Command::new(executable)
        .current_dir(root)
        .env("BOSON_CONFIG", config_path)
        .env("BOSON__DATABASE__CONNECT_ON_BOOT", "true")
        .env("BOSON__DATABASE__RUN_MIGRATIONS", "true")
        .status()
        .await
        .with_context(|| format!("start migration binary {}", executable.display()))?;
    if !status.success() {
        bail!(
            "migrations failed\nfix: run `boson doctor`, then inspect the database and migration files"
        );
    }
    println!("[migrate] migrations current");
    Ok(())
}

/// Starts a project service in its own process group and streams prefixed logs.
///
/// # Errors
///
/// Returns an error when the executable or log stream cannot be opened.
pub async fn spawn_service(
    name: &str,
    executable: &Path,
    root: &Path,
    config_path: &Path,
    log_path: &Path,
) -> Result<ManagedProcess> {
    let mut command = Command::new(executable);
    command
        .current_dir(root)
        .env("BOSON_CONFIG", config_path)
        .env("BOSON__DATABASE__CONNECT_ON_BOOT", "true")
        .env("BOSON__DATABASE__RUN_MIGRATIONS", "false")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(false);
    #[cfg(unix)]
    command.process_group(0);

    let mut child = command
        .spawn()
        .with_context(|| format!("start {name} from {}", executable.display()))?;
    let pid = child.id().context("service exited before startup")?;
    let stdout = child.stdout.take().context("capture service stdout")?;
    let stderr = child.stderr.take().context("capture service stderr")?;
    let log_tasks = vec![
        forward_lines(name.to_owned(), stdout, log_path.to_path_buf()),
        forward_lines(name.to_owned(), stderr, log_path.to_path_buf()),
    ];
    Ok(ManagedProcess {
        name: name.to_owned(),
        pid,
        child,
        log_tasks,
    })
}

fn forward_lines<R>(name: String, stream: R, log_path: PathBuf) -> JoinHandle<()>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let Ok(mut log) = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path)
            .await
        else {
            return;
        };
        let mut lines = BufReader::new(stream).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            println!("[{name}] {line}");
            let _ = log.write_all(format!("{line}\n").as_bytes()).await;
            let _ = log.flush().await;
        }
    })
}

#[cfg(unix)]
fn send_terminate(pid: u32) {
    let Ok(raw_pid) = i32::try_from(pid) else {
        return;
    };
    let _ = nix::sys::signal::killpg(
        nix::unistd::Pid::from_raw(raw_pid),
        nix::sys::signal::Signal::SIGTERM,
    );
}

#[cfg(not(unix))]
fn send_terminate(_pid: u32) {}

/// Waits for a process to remain alive for the startup grace period.
pub async fn ensure_process_started(process: &mut ManagedProcess, grace: Duration) -> Result<()> {
    sleep(grace).await;
    if let Some(status) = process.try_wait()? {
        bail!(
            "{} exited during startup with {status}\nfix: run `boson logs {}`",
            process.name,
            process.name
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_debug_binary_from_cargo_target_directory() {
        assert_eq!(
            executable_path(Path::new("/tmp/target"), "demo_server"),
            Path::new("/tmp/target")
                .join("debug")
                .join(format!("demo_server{}", std::env::consts::EXE_SUFFIX))
        );
    }
}
