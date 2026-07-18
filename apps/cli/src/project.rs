use anyhow::{Result, bail};

pub use boson_orchestration::ProjectManifest;

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

    #[test]
    fn validates_kebab_case_names() {
        assert!(validate_app_name("todo-app").is_ok());
        assert!(validate_app_name("Todo").is_err());
        assert!(validate_app_name("-todo").is_err());
    }
}
