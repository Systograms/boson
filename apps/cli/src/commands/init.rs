use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};

use crate::{
    project::validate_app_name,
    templates::{BosonSource, TemplateContext, render_project},
};

pub struct InitArgs {
    pub name: String,
    pub path: Option<String>,
    pub boson_path: Option<String>,
    pub boson_git: String,
    pub boson_rev: String,
    pub force: bool,
}

pub fn run(args: InitArgs) -> Result<()> {
    validate_app_name(&args.name)?;
    let destination = args
        .path
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&args.name));

    prepare_destination(&destination, args.force)?;

    let source = match args.boson_path {
        Some(path) => {
            let path = PathBuf::from(path)
                .canonicalize()
                .context("resolve --boson-path")?;
            BosonSource::Path(path)
        }
        None => BosonSource::Git {
            url: args.boson_git,
            rev: args.boson_rev,
        },
    };

    let ctx = TemplateContext::new(&args.name, source);
    for file in render_project(&ctx) {
        let path = destination.join(file.relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        fs::write(&path, file.contents).with_context(|| format!("write {}", path.display()))?;
    }

    println!(
        "created Boson app `{}` in {}",
        args.name,
        destination.display()
    );
    println!();
    println!("Next steps:");
    println!("  cd {}", destination.display());
    println!("  boson start");
    Ok(())
}

fn prepare_destination(destination: &Path, force: bool) -> Result<()> {
    if !destination.exists() {
        fs::create_dir_all(destination)
            .with_context(|| format!("create {}", destination.display()))?;
        return Ok(());
    }
    let is_empty = fs::read_dir(destination)
        .with_context(|| format!("read {}", destination.display()))?
        .next()
        .is_none();
    if is_empty {
        return Ok(());
    }
    if !force {
        bail!(
            "destination {} is not empty; pass --force to overwrite generated files",
            destination.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn refuses_non_empty_destination_without_force() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("keep.txt"), "x").unwrap();
        let error = prepare_destination(dir.path(), false).unwrap_err();
        assert!(error.to_string().contains("not empty"));
    }

    #[test]
    fn renders_project_files() {
        let dir = tempdir().unwrap();
        let boson = dir.path().join("boson-src");
        fs::create_dir_all(&boson).unwrap();
        let dest = dir.path().join("demo-app");
        run(InitArgs {
            name: "demo-app".into(),
            path: Some(dest.to_string_lossy().into()),
            boson_path: Some(boson.to_string_lossy().into()),
            boson_git: "https://example.com/boson".into(),
            boson_rev: "main".into(),
            force: false,
        })
        .unwrap();
        assert!(dest.join(".boson/project.toml").is_file());
        assert!(dest.join(".boson/config.yaml").is_file());
        let manifest = fs::read_to_string(dest.join(".boson/project.toml")).unwrap();
        assert!(!manifest.contains("config_path"));
        assert!(!dest.join("config").exists());
        assert!(dest.join("capabilities/items/src/lib.rs").is_file());
        let readme = fs::read_to_string(dest.join("README.md")).unwrap();
        assert!(readme.contains("boson start"));
        assert!(!readme.contains("docker compose"));
        assert!(!dest.join("compose.yaml").exists());
        let cargo = fs::read_to_string(dest.join("apps/server/Cargo.toml")).unwrap();
        assert!(cargo.contains("crates/runtime"));
        let items = fs::read_to_string(dest.join("capabilities/items/Cargo.toml")).unwrap();
        assert!(items.contains("crates/sdk"));
    }
}
