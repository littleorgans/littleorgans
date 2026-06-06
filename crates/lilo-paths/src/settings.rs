//! Operator `settings.toml` config layer.
//!
//! `$LILO_HOME/settings.toml` is the file-based config surface. `LILO_*` env
//! vars override it (see the resolvers in [`crate::env`]); the precedence,
//! highest first, is explicit flag (none today) -> env -> `settings.toml` ->
//! built-in default. Scope is deliberately minimal: only the `[database]` keys
//! flow through it. A missing file is not an error (defaults); a present but
//! malformed file is.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

use crate::{LiloHome, LiloPathError, LiloPaths};

/// Operator configuration loaded from `$LILO_HOME/settings.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// Database connection configuration.
    pub database: DatabaseSettings,
}

/// `[database]` section of `settings.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DatabaseSettings {
    /// Operator Postgres connection URL.
    pub url: Option<String>,
    /// Admin/provisioning Postgres URL for the test fixture. Defaults to `url`.
    pub test_url: Option<String>,
}

impl Settings {
    /// Load `$LILO_HOME/settings.toml`.
    ///
    /// # Errors
    /// Returns an error if the home cannot be resolved or the file is present
    /// but malformed. A missing file yields [`Settings::default`].
    pub fn load() -> Result<Self, SettingsError> {
        let paths = LiloPaths::new(LiloHome::from_env()?);
        Self::load_from(&paths.settings_path())
    }

    /// Load settings from an explicit path (tests / non-default homes).
    ///
    /// # Errors
    /// Returns an error if the file is present but malformed, or unreadable for
    /// a reason other than not existing. A missing file yields the default.
    pub fn load_from(path: &Path) -> Result<Self, SettingsError> {
        match std::fs::read_to_string(path) {
            Ok(contents) => toml::from_str(&contents).map_err(|source| SettingsError::Parse {
                path: path.to_path_buf(),
                source,
            }),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(SettingsError::Read {
                path: path.to_path_buf(),
                source,
            }),
        }
    }
}

/// Failure loading [`Settings`].
#[derive(Debug, Error)]
pub enum SettingsError {
    /// The home directory could not be resolved.
    #[error(transparent)]
    Home(#[from] LiloPathError),
    /// The settings file exists but could not be read.
    #[error("failed to read settings file {}: {source}", path.display())]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The settings file exists but is malformed.
    #[error("failed to parse settings file {}: {source}", path.display())]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_settings_file_yields_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");

        let settings = Settings::load_from(&path).expect("missing file is not an error");
        assert!(settings.database.url.is_none());
        assert!(settings.database.test_url.is_none());
    }

    #[test]
    fn present_settings_file_parses_database_section() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");
        std::fs::write(
            &path,
            "[database]\nurl = \"postgres://lilo:lilo@localhost:55432/lilo\"\n",
        )
        .expect("write settings");

        let settings = Settings::load_from(&path).expect("valid settings parse");
        assert_eq!(
            settings.database.url.as_deref(),
            Some("postgres://lilo:lilo@localhost:55432/lilo")
        );
        assert!(settings.database.test_url.is_none());
    }

    #[test]
    fn malformed_settings_file_errors() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, "[database").expect("write settings");

        let error = Settings::load_from(&path).expect_err("malformed file must error");
        assert!(matches!(error, SettingsError::Parse { .. }));
    }

    #[test]
    fn unknown_key_is_rejected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("settings.toml");
        std::fs::write(&path, "[database]\nhost = \"nope\"\n").expect("write settings");

        let error = Settings::load_from(&path).expect_err("unknown key must error");
        assert!(matches!(error, SettingsError::Parse { .. }));
    }
}
