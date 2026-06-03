use std::ffi::OsString;
use std::path::PathBuf;

/// Operator logging filter.
pub const LILO_LOG: &str = "LILO_LOG";
/// Operator logging formatter override.
pub const LILO_LOG_FORMAT: &str = "LILO_LOG_FORMAT";

pub(crate) fn env_path(name: &str) -> Option<PathBuf> {
    non_empty_env(name).map(PathBuf::from)
}

fn non_empty_env(name: &str) -> Option<OsString> {
    std::env::var_os(name).filter(|value| !value.is_empty())
}
