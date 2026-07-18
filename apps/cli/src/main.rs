mod commands;
mod project;
mod templates;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "boson", version, about = "Run and manage a Boson project")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create a standalone Boson project on infrastructure you own.
    Create {
        name: String,
        #[arg(long)]
        path: Option<String>,
        /// PostgreSQL connection URL written into `.boson/config.yaml`.
        #[arg(long)]
        database_url: Option<String>,
        #[arg(long, hide = true)]
        boson_path: Option<String>,
        #[arg(
            long,
            default_value = "https://github.com/Systograms/boson",
            hide = true
        )]
        boson_git: String,
        #[arg(long, default_value = "main", hide = true)]
        boson_rev: String,
        #[arg(long)]
        force: bool,
    },
    /// Compatibility alias for `boson create`.
    #[command(hide = true)]
    Init {
        name: String,
        #[arg(long)]
        path: Option<String>,
        #[arg(long)]
        database_url: Option<String>,
        #[arg(long, hide = true)]
        boson_path: Option<String>,
        #[arg(
            long,
            default_value = "https://github.com/Systograms/boson",
            hide = true
        )]
        boson_git: String,
        #[arg(long, default_value = "main", hide = true)]
        boson_rev: String,
        #[arg(long)]
        force: bool,
    },
    /// Start the entire Boson project and stream unified logs.
    Start,
    /// Gracefully stop everything started by Boson.
    Stop,
    /// Show services, ports, health, and versions.
    Status,
    /// Tail unified logs or one service.
    Logs {
        service: Option<String>,
        #[arg(short, long)]
        follow: bool,
        #[arg(short = 'n', long, default_value_t = 100)]
        lines: usize,
    },
    /// Check configured connections and print actionable fixes.
    Doctor,
    /// Print the effective configuration with secrets redacted.
    Config,
    /// Package the project into portable container images.
    Deploy {
        /// Directory that receives generated packaging files.
        #[arg(long, default_value = ".boson/deploy")]
        output: String,
        /// Image repository/name prefix, for example `my-registry/my-api`.
        #[arg(long)]
        tag: Option<String>,
        /// Build images with the local Docker CLI after writing packaging files.
        #[arg(long)]
        build: bool,
        /// Push built images to the configured registry. Implies `--build`.
        #[arg(long)]
        push: bool,
    },
    /// Update the Boson binary to the latest release.
    Update {
        #[arg(long)]
        check: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Create {
            name,
            path,
            database_url,
            boson_path,
            boson_git,
            boson_rev,
            force,
        }
        | Command::Init {
            name,
            path,
            database_url,
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
            database_url,
            force,
        }),
        Command::Start => commands::lifecycle::start().await,
        Command::Stop => commands::lifecycle::stop().await,
        Command::Status => commands::lifecycle::status().await,
        Command::Logs {
            service,
            follow,
            lines,
        } => commands::lifecycle::logs(service, follow, lines).await,
        Command::Doctor => commands::doctor::run().await,
        Command::Config => commands::config::run(),
        Command::Deploy {
            output,
            tag,
            build,
            push,
        } => {
            commands::deploy::run(commands::deploy::DeployArgs {
                output,
                tag,
                build: build || push,
                push,
            })
            .await
        }
        Command::Update { check } => commands::update::run(check).await,
    }
}
