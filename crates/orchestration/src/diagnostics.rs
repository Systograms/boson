use std::{
    net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream},
    process::Stdio,
    time::Duration,
};

use boson_db::Database;
use boson_kernel::DatabaseConfig;
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

    diagnostics.push(check_database(&project.config.database).await);
    diagnostics.push(check_port("server port", project.config.http.port));

    diagnostics
}

async fn check_database(config: &DatabaseConfig) -> Diagnostic {
    match Database::connect(config).await {
        Ok(database) => match database.ping().await {
            Ok(()) => Diagnostic::pass("database", "PostgreSQL connection is healthy"),
            Err(error) => Diagnostic::fail(
                "database",
                format!("connected, but health query failed: {error}"),
                database_fix(),
            ),
        },
        Err(error) => Diagnostic::fail(
            "database",
            format!("PostgreSQL connection failed: {error}"),
            database_fix(),
        ),
    }
}

fn database_fix() -> &'static str {
    "verify `database.url` in `.boson/config.yaml` or set `BOSON__DATABASE__URL`; Boson does not start PostgreSQL"
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

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use super::*;

    #[tokio::test]
    async fn database_check_fails_when_configured_postgres_is_unreachable() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let config = DatabaseConfig {
            url: format!("postgres://boson:boson@127.0.0.1:{port}/boson"),
            ..DatabaseConfig::default()
        };

        let diagnostic = check_database(&config).await;

        assert_eq!(diagnostic.name, "database");
        assert_eq!(diagnostic.level, DiagnosticLevel::Fail);
        assert!(diagnostic.message.contains("connection failed"));
        assert!(
            diagnostic
                .fix
                .as_deref()
                .unwrap()
                .contains("BOSON__DATABASE__URL")
        );
    }
}
