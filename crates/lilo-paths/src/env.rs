use std::env;
use std::ffi::OsString;
use std::path::PathBuf;

pub(crate) fn env_path(name: &str) -> Option<PathBuf> {
    non_empty_env(name).map(PathBuf::from)
}

fn non_empty_env(name: &str) -> Option<OsString> {
    env::var_os(name).filter(|value| !value.is_empty())
}
