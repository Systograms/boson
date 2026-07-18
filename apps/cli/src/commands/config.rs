use anyhow::Result;
use boson_orchestration::Project;

pub fn run() -> Result<()> {
    let project = Project::discover(None)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&project.config.redacted())?
    );
    Ok(())
}
