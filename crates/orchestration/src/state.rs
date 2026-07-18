use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UnitKind {
    Infrastructure,
    Process,
    LocalResource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitState {
    pub name: String,
    pub kind: UnitKind,
    pub pid: Option<u32>,
    pub port: Option<u16>,
    pub version: Option<String>,
    pub log_path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleState {
    pub schema_version: u32,
    pub project: String,
    pub supervisor_pid: u32,
    pub started_at: DateTime<Utc>,
    pub config_snapshot: String,
    pub units: Vec<UnitState>,
}

impl LifecycleState {
    #[must_use]
    pub fn new(project: String, config_snapshot: String) -> Self {
        Self {
            schema_version: 1,
            project,
            supervisor_pid: std::process::id(),
            started_at: Utc::now(),
            config_snapshot,
            units: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct StateStore {
    runtime_dir: PathBuf,
    logs_dir: PathBuf,
}

impl StateStore {
    #[must_use]
    pub fn new(runtime_dir: PathBuf, logs_dir: PathBuf) -> Self {
        Self {
            runtime_dir,
            logs_dir,
        }
    }

    /// Creates runtime and log directories.
    ///
    /// # Errors
    ///
    /// Returns an error when the directories cannot be created.
    pub fn initialize(&self) -> Result<()> {
        fs::create_dir_all(&self.runtime_dir)
            .with_context(|| format!("create {}", self.runtime_dir.display()))?;
        fs::create_dir_all(&self.logs_dir)
            .with_context(|| format!("create {}", self.logs_dir.display()))?;
        Ok(())
    }

    #[must_use]
    pub fn state_path(&self) -> PathBuf {
        self.runtime_dir.join("state.json")
    }

    #[must_use]
    pub fn lock_path(&self) -> PathBuf {
        self.runtime_dir.join("lifecycle.lock")
    }

    #[must_use]
    pub fn log_path(&self, unit: &str) -> PathBuf {
        self.logs_dir.join(format!("{unit}.log"))
    }

    /// Starts a fresh log session for the supplied units.
    ///
    /// # Errors
    ///
    /// Returns an error when a log file cannot be created or truncated.
    pub fn reset_logs(&self, units: &[&str]) -> Result<()> {
        self.initialize()?;
        for unit in units {
            let path = self.log_path(unit);
            fs::write(&path, []).with_context(|| format!("reset {}", path.display()))?;
        }
        Ok(())
    }

    /// Atomically saves lifecycle state.
    ///
    /// # Errors
    ///
    /// Returns an error when serialization or filesystem operations fail.
    pub fn save(&self, state: &LifecycleState) -> Result<()> {
        self.initialize()?;
        let temporary = self.runtime_dir.join("state.json.tmp");
        fs::write(&temporary, serde_json::to_vec_pretty(state)?)
            .with_context(|| format!("write {}", temporary.display()))?;
        fs::rename(&temporary, self.state_path()).context("publish lifecycle state")?;
        Ok(())
    }

    /// Loads lifecycle state when it exists.
    ///
    /// # Errors
    ///
    /// Returns an error when state is unreadable or corrupt.
    pub fn load(&self) -> Result<Option<LifecycleState>> {
        let path = self.state_path();
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        let state =
            serde_json::from_slice(&raw).with_context(|| format!("parse {}", path.display()))?;
        Ok(Some(state))
    }

    /// Removes runtime state without deleting logs.
    ///
    /// # Errors
    ///
    /// Returns an error when the state file exists but cannot be removed.
    pub fn clear(&self) -> Result<()> {
        let path = self.state_path();
        if path.exists() {
            fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
        }
        Ok(())
    }
}

#[must_use]
pub fn process_is_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let Ok(raw_pid) = i32::try_from(pid) else {
            return false;
        };
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(raw_pid), None).is_ok()
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

pub fn read_last_lines(path: &Path, lines: usize) -> Result<Vec<String>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents =
        fs::read_to_string(path).with_context(|| format!("read log file {}", path.display()))?;
    let all = contents.lines().collect::<Vec<_>>();
    let start = all.len().saturating_sub(lines);
    Ok(all[start..].iter().map(|line| (*line).to_owned()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn state_round_trip_is_atomic() {
        let dir = tempdir().unwrap();
        let store = StateStore::new(dir.path().join("run"), dir.path().join("logs"));
        let state = LifecycleState::new("demo".into(), "snapshot".into());
        store.save(&state).unwrap();
        let loaded = store.load().unwrap().unwrap();
        assert_eq!(loaded.project, "demo");
        store.clear().unwrap();
        assert!(store.load().unwrap().is_none());
    }

    #[test]
    fn tails_requested_number_of_lines() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("app.log");
        fs::write(&path, "one\ntwo\nthree\n").unwrap();
        assert_eq!(read_last_lines(&path, 2).unwrap(), vec!["two", "three"]);
    }

    #[test]
    fn reset_logs_removes_previous_session_output() {
        let dir = tempdir().unwrap();
        let store = StateStore::new(dir.path().join("run"), dir.path().join("logs"));
        store.initialize().unwrap();
        fs::write(store.log_path("server"), "old failure\n").unwrap();
        store.reset_logs(&["server"]).unwrap();
        assert!(
            read_last_lines(&store.log_path("server"), 10)
                .unwrap()
                .is_empty()
        );
    }
}
