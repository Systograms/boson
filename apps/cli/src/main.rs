mod client;
mod commands;
mod project;
mod templates;

use anyhow::Result;
use clap::{Parser, Subcommand};
use client::AdminClient;

#[derive(Debug, Parser)]
#[command(name = "boson", version, about = "Boson platform developer CLI")]
struct Cli {
    #[arg(long, env = "BOSON_URL", default_value = "http://localhost:8080")]
    server: String,
    #[arg(long, env = "BOSON_ADMIN_TOKEN")]
    admin_token: Option<String>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Check prerequisites, project metadata, and server health.
    Doctor,
    /// Read the redacted effective server configuration.
    Config,
    /// Print the server's operational overview.
    Overview,
    /// Scaffold a standalone Boson application workspace.
    Init {
        /// Application name (kebab-case).
        name: String,
        /// Destination directory. Defaults to `./<name>`.
        #[arg(long)]
        path: Option<String>,
        /// Path to a local Boson checkout used for path dependencies.
        #[arg(long)]
        boson_path: Option<String>,
        /// Git URL for Boson dependencies. Ignored when `--boson-path` is set.
        #[arg(long, default_value = "https://github.com/Systograms/boson")]
        boson_git: String,
        /// Git branch/tag/rev for `--boson-git`.
        #[arg(long, default_value = "main")]
        boson_rev: String,
        /// Overwrite an existing non-empty destination.
        #[arg(long)]
        force: bool,
    },
    /// Apply platform and project capability migrations.
    ///
    /// This is a narrow operational exception to the CLI's otherwise API-only
    /// rule: migrations must touch PostgreSQL directly.
    Migrate {
        /// Config file path. Defaults to `BOSON_CONFIG` or `config/local.yaml`.
        #[arg(long)]
        config: Option<String>,
    },
    /// Start Postgres (via Compose), migrate, then run server and worker.
    Dev {
        /// Config file path. Defaults to `BOSON_CONFIG` or `config/local.yaml`.
        #[arg(long)]
        config: Option<String>,
        /// Skip starting Compose Postgres.
        #[arg(long)]
        no_db: bool,
    },
    /// Manage persistent platform administrator identities.
    Admin {
        #[command(subcommand)]
        command: AdminCommand,
    },
}

#[derive(Debug, Subcommand)]
enum AdminCommand {
    /// Create an administrator and issue its first API key.
    Create {
        #[arg(long)]
        email: String,
        #[arg(long)]
        display_name: String,
        #[arg(long, default_value = "default")]
        key_name: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = AdminClient::new(cli.server.clone(), cli.admin_token.clone());
    match cli.command {
        Command::Doctor => commands::doctor::run(&client).await,
        Command::Config => {
            let body = client.get("config").await?;
            println!("{}", serde_json::to_string_pretty(&body)?);
            Ok(())
        }
        Command::Overview => {
            let body = client.get("overview").await?;
            println!("{}", serde_json::to_string_pretty(&body)?);
            Ok(())
        }
        Command::Init {
            name,
            path,
            boson_path,
            boson_git,
            boson_rev,
            force,
        } => commands::init::run(commands::init::InitArgs {
            name,
            path,
            boson_path,
            boson_git,
            boson_rev,
            force,
        }),
        Command::Migrate { config } => commands::migrate::run(config).await,
        Command::Dev { config, no_db } => commands::dev::run(config, no_db).await,
        Command::Admin {
            command:
                AdminCommand::Create {
                    email,
                    display_name,
                    key_name,
                },
        } => {
            let body = client
                .post(
                    "admins",
                    serde_json::json!({
                        "email": email,
                        "display_name": display_name,
                        "key_name": key_name
                    }),
                )
                .await?;
            println!("{}", serde_json::to_string_pretty(&body)?);
            Ok(())
        }
    }
}
