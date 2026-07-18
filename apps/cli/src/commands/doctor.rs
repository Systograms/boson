use anyhow::Result;
use boson_orchestration::{DiagnosticLevel, Project, run_diagnostics};

pub async fn run() -> Result<()> {
    let project = Project::discover(None)?;
    let diagnostics = run_diagnostics(&project).await;
    let mut failed = false;
    println!("Boson doctor");
    println!();
    for diagnostic in diagnostics {
        let marker = match diagnostic.level {
            DiagnosticLevel::Pass => "OK",
            DiagnosticLevel::Warning => "WARN",
            DiagnosticLevel::Fail => {
                failed = true;
                "FAIL"
            }
        };
        println!("[{marker}] {:<18} {}", diagnostic.name, diagnostic.message);
        if let Some(fix) = diagnostic.fix {
            println!("       fix: {fix}");
        }
    }
    println!();
    if failed {
        anyhow::bail!("one or more required checks failed; apply the fixes above");
    }
    println!("Boson can start this project");
    Ok(())
}
