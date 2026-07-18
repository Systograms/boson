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
    /// Create a minimal standalone Boson project.
    Init {
        name: String,
        #[arg(long)]
        path: Option<String>,
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
    /// Check the machine and print actionable fixes.
    Doctor,
    /// Print the effective configuration with secrets redacted.
    Config,
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
        Command::Update { check } => commands::update::run(check).await,
    }
}
