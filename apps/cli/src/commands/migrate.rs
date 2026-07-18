use std::{env, path::PathBuf, process::Command};

use anyhow::{Context, Result, bail};
use boson_db::Database;
use boson_kernel::PlatformConfig;
use boson_runtime::run_migrations;

use crate::project::find_project_root;

pub async fn run(config: Option<String>) -> Result<()> {
    if let Some((root, manifest)) = find_project_root(None)? {
        println!(
            "migrating project `{}` via {}",
            manifest.name, manifest.migrate_package
        );
        let status = Command::new("cargo")
            .arg("run")
            .arg("-p")
            .arg(&manifest.migrate_package)
            .arg("--quiet")
            .current_dir(&root)
            .env(
                "BOSON_CONFIG",
                config.unwrap_or_else(|| {
                    env::var("BOSON_CONFIG").unwrap_or_else(|_| "config/local.yaml".into())
                }),
            )
            .status()
            .context("run project migrate binary")?;
        if !status.success() {
            bail!("project migrate failed with {status}");
        }
        println!("migrations applied");
        return Ok(());
    }

    let config_path = config
        .map(PathBuf::from)
        .or_else(|| env::var("BOSON_CONFIG").ok().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("config/local.yaml"));
    let mut platform = PlatformConfig::load(&config_path)?;
    platform.database.connect_on_boot = true;
    let database = Database::connect(&platform.database)
        .await
        .context("connect to PostgreSQL for migrations")?;
    run_migrations(&database, None, &[])
        .await
        .context("apply embedded platform migrations")?;
    println!("platform migrations applied");
    Ok(())
}
