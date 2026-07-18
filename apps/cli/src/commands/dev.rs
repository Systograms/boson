use std::{
    env,
    path::Path,
    process::{Child, Command, Stdio},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use tokio::time::sleep;

use crate::project::require_project_root;

pub async fn run(config: Option<String>, no_db: bool) -> Result<()> {
    let (root, manifest) = require_project_root(None)?;
    let config_path = config
        .unwrap_or_else(|| env::var("BOSON_CONFIG").unwrap_or_else(|_| "config/local.yaml".into()));

    if !no_db {
        ensure_postgres(&root).await?;
    }

    println!("applying migrations...");
    let migrate_status = Command::new("cargo")
        .arg("run")
        .arg("-p")
        .arg(&manifest.migrate_package)
        .arg("--quiet")
        .current_dir(&root)
        .env("BOSON_CONFIG", &config_path)
        .env("BOSON__DATABASE__CONNECT_ON_BOOT", "true")
        .env("BOSON__DATABASE__RUN_MIGRATIONS", "true")
        .status()
        .context("run migrate")?;
    if !migrate_status.success() {
        bail!("migrate failed with {migrate_status}");
    }

    println!(
        "starting {} and {}...",
        manifest.server_package, manifest.worker_package
    );
    let mut server = spawn_package(&root, &manifest.server_package, &config_path, "server")?;
    let mut worker = spawn_package(&root, &manifest.worker_package, &config_path, "worker")?;

    if let Err(error) = wait_for_ready().await {
        let _ = server.kill();
        let _ = worker.kill();
        return Err(error);
    }
    println!("dev stack ready at http://localhost:8080");
    println!("press Ctrl+C to stop");

    let result = tokio::signal::ctrl_c().await;
    let _ = server.kill();
    let _ = worker.kill();
    let _ = server.wait();
    let _ = worker.wait();
    result.context("wait for shutdown signal")?;
    println!("stopped");
    Ok(())
}

async fn ensure_postgres(root: &Path) -> Result<()> {
    let compose = root.join("compose.yaml");
    if !compose.exists() {
        println!("compose.yaml missing; assuming Postgres is already available");
        return Ok(());
    }
    println!("starting Postgres via docker compose...");
    let status = Command::new("docker")
        .args(["compose", "up", "-d", "postgres"])
        .current_dir(root)
        .status()
        .context("docker compose up postgres")?;
    if !status.success() {
        bail!("docker compose failed with {status}");
    }
    for _ in 0..40 {
        let ready = Command::new("docker")
            .args([
                "compose",
                "exec",
                "-T",
                "postgres",
                "pg_isready",
                "-U",
                "boson",
            ])
            .current_dir(root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if ready {
            println!("Postgres is ready");
            return Ok(());
        }
        sleep(Duration::from_millis(500)).await;
    }
    bail!("Postgres did not become ready in time");
}

fn spawn_package(root: &Path, package: &str, config_path: &str, label: &str) -> Result<Child> {
    let mut command = Command::new("cargo");
    command
        .arg("run")
        .arg("-p")
        .arg(package)
        .arg("--quiet")
        .current_dir(root)
        .env("BOSON_CONFIG", config_path)
        .env("BOSON__DATABASE__CONNECT_ON_BOOT", "true")
        .env("BOSON__DATABASE__RUN_MIGRATIONS", "false")
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let child = command
        .spawn()
        .with_context(|| format!("spawn {label} ({package})"))?;
    Ok(child)
}

async fn wait_for_ready() -> Result<()> {
    let client = reqwest::Client::new();
    for _ in 0..60 {
        if let Ok(response) = client.get("http://localhost:8080/readyz").send().await
            && response.status().is_success()
        {
            return Ok(());
        }
        sleep(Duration::from_millis(500)).await;
    }
    bail!("server did not become ready at http://localhost:8080/readyz");
}
