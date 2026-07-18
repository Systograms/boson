use std::{env, time::Duration};

use anyhow::{Context, Result, bail};
use boson_db::Database;
use boson_kernel::{PlatformConfig, init_telemetry};

#[tokio::main]
async fn main() -> Result<()> {
    let config_path = env::var("BOSON_CONFIG").unwrap_or_else(|_| "config/local.yaml".to_owned());
    let config = PlatformConfig::load(config_path)?;
    init_telemetry(&config.telemetry)?;

    if !config.database.connect_on_boot {
        bail!("worker requires database.connect_on_boot=true");
    }

    let database = Database::connect(&config.database)
        .await
        .context("connect worker to PostgreSQL")?;
    if config.database.run_migrations {
        database.migrate("migrations").await?;
    }

    tracing::info!("Boson worker started");
    let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
    loop {
        tokio::select! {
            _ = heartbeat.tick() => {
                if let Err(error) = database.heartbeat("default").await {
                    tracing::error!(%error, "failed to record worker heartbeat");
                } else {
                    tracing::debug!("worker heartbeat recorded");
                }
            }
            signal = tokio::signal::ctrl_c() => {
                signal.context("listen for worker shutdown")?;
                tracing::info!("worker shutdown signal received");
                break;
            }
        }
    }
    Ok(())
}
