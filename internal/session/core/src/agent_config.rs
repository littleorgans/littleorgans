use std::path::{Component, Path, PathBuf};

use lilo_paths::expand_home_path;

pub fn is_agent_config_path_like(value: &str) -> bool {
    value.contains(std::path::MAIN_SEPARATOR) || value.starts_with('~') || value.starts_with('.')
}

pub fn agent_config_uses_home_prefix(value: &str) -> bool {
    value == "~" || value.starts_with("~/")
}

pub fn normalize_agent_config_request(
    value: &str,
    base_dir: &Path,
    home_dir: Option<&Path>,
) -> String {
    if !is_agent_config_path_like(value) {
        return value.to_string();
    }

    let path = expand_home_path(value, home_dir).unwrap_or_else(|| PathBuf::from(value));
    let absolute = if path.is_absolute() {
        path
    } else {
        base_dir.join(path)
    };
    std::fs::canonicalize(&absolute)
        .unwrap_or_else(|_| normalize_lexically(&absolute))
        .display()
        .to_string()
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir if can_pop(&normalized) => {
                normalized.pop();
            }
            Component::ParentDir => normalized.push(component.as_os_str()),
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        normalized
    }
}

fn can_pop(path: &Path) -> bool {
    matches!(path.components().next_back(), Some(Component::Normal(_)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_toml_filename_is_not_path_like() {
        assert!(!is_agent_config_path_like("tools.toml"));
    }

    #[test]
    fn relative_dot_path_is_path_like() {
        assert!(is_agent_config_path_like("./tools.toml"));
    }

    #[test]
    fn separator_relative_path_is_path_like() {
        let value = format!("configs{}tools.toml", std::path::MAIN_SEPARATOR);

        assert!(is_agent_config_path_like(&value));
    }

    #[test]
    fn absolute_path_is_path_like() {
        let value = format!(
            "{}abs{}x.toml",
            std::path::MAIN_SEPARATOR,
            std::path::MAIN_SEPARATOR
        );

        assert!(is_agent_config_path_like(&value));
    }

    #[test]
    fn home_relative_path_is_path_like() {
        assert!(is_agent_config_path_like("~/x.toml"));
    }

    #[test]
    fn bare_name_is_not_path_like() {
        assert!(!is_agent_config_path_like("demo"));
    }

    #[test]
    fn bare_agent_config_name_is_not_normalized() {
        let base = absolute_fixture_path("workspace");

        assert_eq!(normalize_agent_config_request("demo", &base, None), "demo");
    }

    #[test]
    fn relative_agent_config_path_is_normalized_against_base() {
        let base = absolute_fixture_path("workspace");

        assert_eq!(
            normalize_agent_config_request("./agent.toml", &base, None),
            base.join("agent.toml").display().to_string()
        );
    }

    #[test]
    fn parent_components_are_removed_from_missing_agent_config_path() {
        let base = absolute_fixture_path("workspace").join("child");

        assert_eq!(
            normalize_agent_config_request("../agent.toml", &base, None),
            absolute_fixture_path("workspace")
                .join("agent.toml")
                .display()
                .to_string()
        );
    }

    #[test]
    fn home_relative_agent_config_path_uses_supplied_home() {
        let base = absolute_fixture_path("workspace");
        let home = absolute_fixture_path("home");

        assert_eq!(
            normalize_agent_config_request("~/agent.toml", &base, Some(&home)),
            home.join("agent.toml").display().to_string()
        );
    }

    #[test]
    fn home_prefix_is_identified_without_matching_tilde_user() {
        assert!(agent_config_uses_home_prefix("~"));
        assert!(agent_config_uses_home_prefix("~/agent.toml"));
        assert!(!agent_config_uses_home_prefix("~agent/agent.toml"));
    }

    fn absolute_fixture_path(child: &str) -> PathBuf {
        PathBuf::from(std::path::MAIN_SEPARATOR.to_string()).join(child)
    }
}
