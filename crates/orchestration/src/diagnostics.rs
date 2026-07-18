use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream},
    process::Stdio,
    time::Duration,
};

use tokio::process::Command;

use crate::project::Project;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticLevel {
    Pass,
    Warning,
    Fail,
}

#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub name: String,
    pub level: DiagnosticLevel,
    pub message: String,
    pub fix: Option<String>,
}

impl Diagnostic {
    fn pass(name: &str, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            level: DiagnosticLevel::Pass,
            message: message.into(),
            fix: None,
        }
    }

    fn fail(name: &str, message: impl Into<String>, fix: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            level: DiagnosticLevel::Fail,
            message: message.into(),
            fix: Some(fix.into()),
        }
    }
}

pub async fn run(project: &Project) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    diagnostics.push(Diagnostic::pass(
        "project",
        format!("{} ({})", project.manifest.name, project.root.display()),
    ));
    diagnostics.push(Diagnostic::pass(
        "config",
        format!(
            "{} · snapshot {}",
            project.config_path.display(),
            project.config.snapshot_id()
        ),
    ));

    diagnostics.push(
        check_command(
            "rust",
            "cargo",
            &["--version"],
            "install Rust from https://rustup.rs",
        )
        .await,
    );

    diagnostics.push(check_port("server port", project.config.http.port));

    diagnostics
}

async fn check_command(name: &str, command: &str, args: &[&str], fix: &str) -> Diagnostic {
    match Command::new(command)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
    {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            Diagnostic::pass(name, stdout.trim())
        }
        Ok(output) => Diagnostic::fail(name, String::from_utf8_lossy(&output.stderr).trim(), fix),
        Err(error) => Diagnostic::fail(name, error.to_string(), fix),
    }
}

fn check_port(name: &str, port: u16) -> Diagnostic {
    let listening = TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        Duration::from_millis(100),
    )
    .is_ok();
    let ipv4_available = TcpListener::bind((Ipv4Addr::UNSPECIFIED, port)).is_ok();
    let ipv6_available = TcpListener::bind((Ipv6Addr::UNSPECIFIED, port)).is_ok();
    if listening || !ipv4_available || !ipv6_available {
        Diagnostic {
            name: name.into(),
            level: DiagnosticLevel::Warning,
            message: format!("{port} is already in use"),
            fix: Some(
                "run `boson status`; if Boson is stopped, close the process using this port".into(),
            ),
        }
    } else {
        Diagnostic::pass(name, format!("{port} is available"))
    }
}
