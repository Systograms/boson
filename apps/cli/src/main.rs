use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use reqwest::Client;
use serde_json::Value;

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
    /// Check public liveness and readiness endpoints.
    Doctor,
    /// Read the redacted effective server configuration.
    Config,
    /// Print the server's operational overview.
    Overview,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = Client::new();
    match cli.command {
        Command::Doctor => doctor(&client, &cli.server).await,
        Command::Config => admin_get(&client, &cli.server, cli.admin_token, "config").await,
        Command::Overview => admin_get(&client, &cli.server, cli.admin_token, "overview").await,
    }
}

async fn doctor(client: &Client, server: &str) -> Result<()> {
    for endpoint in ["healthz", "readyz"] {
        let url = format!("{}/{endpoint}", server.trim_end_matches('/'));
        let response = client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("cannot connect to {url}; is boson-server running?"))?;
        let status = response.status();
        let body: Value = response.json().await?;
        println!("{endpoint}: {status} {body}");
        if !status.is_success() {
            bail!("{endpoint} check failed");
        }
    }
    Ok(())
}

async fn admin_get(
    client: &Client,
    server: &str,
    token: Option<String>,
    endpoint: &str,
) -> Result<()> {
    let token =
        token.context("admin token required; pass --admin-token or set BOSON_ADMIN_TOKEN")?;
    let url = format!("{}/admin/v1/{endpoint}", server.trim_end_matches('/'));
    let response = client.get(url).bearer_auth(token).send().await?;
    let status = response.status();
    let body: Value = response.json().await?;
    println!("{}", serde_json::to_string_pretty(&body)?);
    if !status.is_success() {
        bail!("Admin API returned {status}");
    }
    Ok(())
}
