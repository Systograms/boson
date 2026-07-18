use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use boson_kernel::PlatformConfig;
use serde::{Deserialize, Serialize};

const PROJECT_MARKER: &str = ".boson/project.toml";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct ProjectManifest {
    pub schema_version: u32,
    pub name: String,
    pub package_prefix: String,
    pub migrate_package: String,
    pub server_package: String,
    pub worker_package: String,
}

impl Default for ProjectManifest {
    fn default() -> Self {
        Self {
            schema_version: 1,
            name: String::new(),
            package_prefix: String::new(),
            migrate_package: String::new(),
            server_package: String::new(),
            worker_package: String::new(),
        }
    }
}

impl ProjectManifest {
    #[must_use]
    pub fn for_name(name: &str) -> Self {
        let package_prefix = name.replace('-', "_");
        Self {
            name: name.to_owned(),
            package_prefix: package_prefix.clone(),
            migrate_package: format!("{package_prefix}_migrate"),
            server_package: format!("{package_prefix}_server"),
            worker_package: format!("{package_prefix}_worker"),
            ..Self::default()
        }
    }

    /// Reads a project manifest and fills package defaults.
    ///
    /// # Errors
    ///
    /// Returns an error when the marker cannot be read, parsed, or validated.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let raw = fs::read_to_string(path)
            .with_context(|| format!("read project marker {}", path.display()))?;
        let mut manifest: Self = toml::from_str(&raw)
            .with_context(|| format!("parse project marker {}", path.display()))?;
        manifest.fill_defaults();
        manifest.validate()?;
        Ok(manifest)
    }

    fn fill_defaults(&mut self) {
        if self.package_prefix.is_empty() {
            self.package_prefix = self.name.replace('-', "_");
        }
        if self.migrate_package.is_empty() {
            self.migrate_package = format!("{}_migrate", self.package_prefix);
        }
        if self.server_package.is_empty() {
            self.server_package = format!("{}_server", self.package_prefix);
        }
        if self.worker_package.is_empty() {
            self.worker_package = format!("{}_worker", self.package_prefix);
        }
    }

    fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            self.schema_version == 1,
            "unsupported Boson project schema {}; update the Boson CLI",
            self.schema_version
        );
        anyhow::ensure!(!self.name.trim().is_empty(), "project name is required");
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Project {
    pub root: PathBuf,
    pub manifest: ProjectManifest,
    pub config_path: PathBuf,
    pub config: PlatformConfig,
}

impl Project {
    /// Discovers and loads a Boson project.
    ///
    /// # Errors
    ///
    /// Returns an actionable error when no marker exists or configuration is invalid.
    pub fn discover(start: Option<&Path>) -> Result<Self> {
        let (root, manifest) = find_project_root(start)?.context(
            "not inside a Boson project\nfix: run `boson init <name>` or change into a project directory",
        )?;
        let config_path = root.join(".boson/config.yaml");
        anyhow::ensure!(
            config_path.is_file(),
            "project configuration is missing at {}\nfix: create `.boson/config.yaml` or run `boson init`",
            config_path.display()
        );
        let config = PlatformConfig::load(&config_path).with_context(|| {
            format!(
                "invalid project configuration {}\nfix: run `boson doctor` for details",
                config_path.display()
            )
        })?;
        Ok(Self {
            root,
            manifest,
            config_path,
            config,
        })
    }

    #[must_use]
    pub fn runtime_dir(&self) -> PathBuf {
        self.root.join(".boson/run")
    }

    #[must_use]
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join(".boson/logs")
    }

    #[must_use]
    pub fn server_url(&self) -> String {
        let host = match self.config.http.host.as_str() {
            "0.0.0.0" | "::" => "127.0.0.1",
            host => host,
        };
        format!("http://{host}:{}", self.config.http.port)
    }
}

/// Walks upward looking for `.boson/project.toml`.
///
/// # Errors
///
/// Returns an error when the current directory or marker cannot be read.
pub fn find_project_root(start: Option<&Path>) -> Result<Option<(PathBuf, ProjectManifest)>> {
    let mut current = start.map_or_else(
        || env::current_dir().context("resolve current directory"),
        |path| Ok(path.to_path_buf()),
    )?;
    loop {
        let candidate = current.join(PROJECT_MARKER);
        if candidate.is_file() {
            return Ok(Some((current, ProjectManifest::load(candidate)?)));
        }
        if !current.pop() {
            return Ok(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn discovers_project_from_nested_directory() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("capabilities/items/src");
        fs::create_dir_all(dir.path().join(".boson")).unwrap();
        fs::create_dir_all(&nested).unwrap();
        fs::write(
            dir.path().join(PROJECT_MARKER),
            toml::to_string(&ProjectManifest::for_name("demo")).unwrap(),
        )
        .unwrap();
        let (root, manifest) = find_project_root(Some(&nested)).unwrap().unwrap();
        assert_eq!(root, dir.path());
        assert_eq!(manifest.server_package, "demo_server");
    }

    #[test]
    fn project_discovery_requires_canonical_config_path() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".boson")).unwrap();
        fs::create_dir_all(dir.path().join("config")).unwrap();
        fs::write(
            dir.path().join(PROJECT_MARKER),
            r#"
schema_version = 1
name = "demo"
config_path = "config/local.yaml"
"#,
        )
        .unwrap();
        fs::write(
            dir.path().join("config/local.yaml"),
            "app:\n  name: legacy\n",
        )
        .unwrap();

        let error = Project::discover(Some(dir.path())).unwrap_err();

        assert!(error.to_string().contains(".boson/config.yaml"));
    }
}
