use std::path::{Path, PathBuf};

pub const DEFAULT_AUDIT_DB_DIR: &str = ".im";
pub const DEFAULT_AUDIT_DB_FILE: &str = "audit.sqlite";

#[must_use]
pub fn default_audit_db_path() -> PathBuf {
    home_dir()
        .join(DEFAULT_AUDIT_DB_DIR)
        .join(DEFAULT_AUDIT_DB_FILE)
}

#[must_use]
pub fn audit_db_parent(path: &Path) -> Option<&Path> {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from)
}
