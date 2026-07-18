use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectManifest {
    pub name: String,
    pub package_prefix: String,
    #[serde(default = "default_migrate_package")]
    pub migrate_package: String,
    #[serde(default = "default_server_package")]
    pub server_package: String,
    #[serde(default = "default_worker_package")]
    pub worker_package: String,
}

fn default_migrate_package() -> String {
    String::new()
}
fn default_server_package() -> String {
    String::new()
}
fn default_worker_package() -> String {
    String::new()
}

impl ProjectManifest {
    pub fn for_name(name: &str) -> Self {
        let package_prefix = name.replace('-', "_");
        Self {
            name: name.to_owned(),
            package_prefix: package_prefix.clone(),
            migrate_package: format!("{package_prefix}_migrate"),
            server_package: format!("{package_prefix}_server"),
            worker_package: format!("{package_prefix}_worker"),
        }
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let raw = fs::read_to_string(path.as_ref())
            .with_context(|| format!("read {}", path.as_ref().display()))?;
        let mut manifest: Self = toml::from_str(&raw).context("parse .boson/project.toml")?;
        if manifest.migrate_package.is_empty() {
            manifest.migrate_package = format!("{}_migrate", manifest.package_prefix);
        }
        if manifest.server_package.is_empty() {
            manifest.server_package = format!("{}_server", manifest.package_prefix);
        }
        if manifest.worker_package.is_empty() {
            manifest.worker_package = format!("{}_worker", manifest.package_prefix);
        }
        Ok(manifest)
    }
}

/// Walks from `start` (or the current directory) upward looking for `.boson/project.toml`.
pub fn find_project_root(start: Option<&Path>) -> Result<Option<(PathBuf, ProjectManifest)>> {
    let mut current = match start {
        Some(path) => path.to_path_buf(),
        None => env::current_dir().context("resolve current directory")?,
    };
    loop {
        let candidate = current.join(".boson/project.toml");
        if candidate.is_file() {
            let manifest = ProjectManifest::load(&candidate)?;
            return Ok(Some((current, manifest)));
        }
        if !current.pop() {
            return Ok(None);
        }
    }
}

pub fn require_project_root(start: Option<&Path>) -> Result<(PathBuf, ProjectManifest)> {
    find_project_root(start)?
        .context("no Boson project found; run this inside an app created by `boson init`")
}

pub fn validate_app_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
        || name.starts_with('-')
        || name.ends_with('-')
        || name.contains("--")
    {
        bail!("app name must be kebab-case (lowercase letters, digits, and single hyphens)");
    }
    Ok(())
}

pub fn to_snake_case(name: &str) -> String {
    name.replace('-', "_")
}

pub fn to_pascal_case(name: &str) -> String {
    name.split('-')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn validates_kebab_case_names() {
        assert!(validate_app_name("todo-app").is_ok());
        assert!(validate_app_name("Todo").is_err());
        assert!(validate_app_name("-todo").is_err());
    }

    #[test]
    fn finds_project_root() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("apps/server");
        fs::create_dir_all(dir.path().join(".boson")).unwrap();
        fs::create_dir_all(&nested).unwrap();
        let manifest = ProjectManifest::for_name("demo");
        fs::write(
            dir.path().join(".boson/project.toml"),
            toml::to_string(&manifest).unwrap(),
        )
        .unwrap();
        let (root, loaded) = find_project_root(Some(&nested)).unwrap().unwrap();
        assert_eq!(root, dir.path());
        assert_eq!(loaded.name, "demo");
    }
}
