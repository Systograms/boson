use anyhow::Result;
use boson_orchestration::{LifecycleManager, Project};

pub async fn start() -> Result<()> {
    let project = Project::discover(None)?;
    LifecycleManager::new(project).start().await
}

pub async fn stop() -> Result<()> {
    let project = Project::discover(None)?;
    LifecycleManager::new(project).stop().await
}

pub async fn status() -> Result<()> {
    let project = Project::discover(None)?;
    let project_name = project.manifest.name.clone();
    let entries = LifecycleManager::new(project).status().await?;
    println!("Boson project: {project_name}");
    println!(
        "{:<15} {:<10} {:<8} {:<14} VERSION",
        "SERVICE", "STATE", "PORT", "HEALTH"
    );
    for entry in entries {
        println!(
            "{:<15} {:<10} {:<8} {:<14} {}",
            entry.name,
            entry.state,
            entry
                .port
                .map_or_else(|| "-".into(), |port| port.to_string()),
            entry.health,
            entry.version.as_deref().unwrap_or("-")
        );
    }
    Ok(())
}

pub async fn logs(unit: Option<String>, follow: bool, lines: usize) -> Result<()> {
    let project = Project::discover(None)?;
    LifecycleManager::new(project)
        .logs(unit.as_deref(), follow, lines)
        .await
}
