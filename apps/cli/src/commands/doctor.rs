use std::{env, process::Command, time::Duration};

use anyhow::Result;
use boson_kernel::PlatformConfig;

use crate::{client::AdminClient, project::find_project_root};

pub async fn run(client: &AdminClient) -> Result<()> {
    println!("boson doctor");
    println!();

    check_command("rustc", &["--version"]);
    check_command("cargo", &["--version"]);
    check_command("docker", &["version", "--format", "{{.Server.Version}}"]);

    match find_project_root(None)? {
        Some((root, manifest)) => {
            println!("project: {} ({})", manifest.name, root.display());
            println!(
                "  packages: {}, {}, {}",
                manifest.server_package, manifest.worker_package, manifest.migrate_package
            );
        }
        None => println!("project: (none in current directory tree)"),
    }

    let config_path = env::var("BOSON_CONFIG").unwrap_or_else(|_| "config/local.yaml".into());
    match PlatformConfig::load(&config_path) {
        Ok(config) => {
            println!(
                "config: loaded {} (app={}, env={})",
                config_path, config.app.name, config.app.environment
            );
            println!(
                "  database.connect_on_boot={} run_migrations={}",
                config.database.connect_on_boot, config.database.run_migrations
            );
        }
        Err(error) => println!("config: failed to load {config_path}: {error}"),
    }

    for endpoint in ["healthz", "readyz"] {
        match client.get_public(endpoint).await {
            Ok((status, body)) => {
                if status.is_success() {
                    println!("{endpoint}: ok {body}");
                } else {
                    println!("{endpoint}: failed ({status}) {body}");
                }
            }
            Err(error) => println!("{endpoint}: unreachable ({error})"),
        }
    }

    if client.admin_token().is_some() {
        match tokio::time::timeout(Duration::from_secs(5), client.get("health")).await {
            Ok(Ok(body)) => println!("admin health: ok {body}"),
            Ok(Err(error)) => println!("admin health: failed ({error})"),
            Err(_) => println!("admin health: timed out"),
        }
    } else {
        println!("admin health: skipped (no --admin-token / BOSON_ADMIN_TOKEN)");
    }

    Ok(())
}

fn check_command(name: &str, args: &[&str]) {
    match Command::new(name).args(args).output() {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let line = stdout.lines().next().or_else(|| stderr.lines().next());
            println!("{name}: {}", line.unwrap_or("ok"));
        }
        Ok(output) => {
            println!("{name}: failed ({})", output.status);
        }
        Err(error) => println!("{name}: missing ({error})"),
    }
}
