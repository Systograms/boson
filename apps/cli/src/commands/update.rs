use anyhow::{Context, Result, bail};
use serde::Deserialize;
use tokio::process::Command;

const RELEASES_URL: &str = "https://api.github.com/repos/Systograms/boson/releases/latest";
const REPOSITORY_URL: &str = "https://github.com/Systograms/boson";

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
}

pub async fn run(check_only: bool) -> Result<()> {
    println!("[update] checking for a newer Boson release");
    let response = reqwest::Client::new()
        .get(RELEASES_URL)
        .header("user-agent", format!("boson/{}", env!("CARGO_PKG_VERSION")))
        .send()
        .await
        .context("could not check for updates; verify your network connection")?;
    if !response.status().is_success() {
        bail!(
            "no installable Boson release was found ({})\nfix: try again later or visit {REPOSITORY_URL}/releases",
            response.status()
        );
    }
    let release: Release = response.json().await.context("read release metadata")?;
    let current = format!("v{}", env!("CARGO_PKG_VERSION"));
    if release.tag_name == current {
        println!("[update] Boson {current} is current");
        return Ok(());
    }
    println!(
        "[update] current {current} · available {}",
        release.tag_name
    );
    if check_only {
        return Ok(());
    }

    println!("[update] installing {}", release.tag_name);
    let status = Command::new("cargo")
        .args([
            "install",
            "--git",
            REPOSITORY_URL,
            "--tag",
            &release.tag_name,
            "boson-cli",
            "--force",
        ])
        .status()
        .await
        .context("could not run the Boson updater")?;
    if !status.success() {
        bail!(
            "Boson update failed\nfix: check your network and Rust installation, then retry `boson update`"
        );
    }
    println!("[update] Boson {} installed", release.tag_name);
    Ok(())
}
