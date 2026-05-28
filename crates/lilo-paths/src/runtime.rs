//! Runtime endpoint helpers that remain after the `~/.lilo` path cutover.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum RuntimeEndpoint {
    UnixSocket(PathBuf),
    WindowsNamedPipe(String),
}

impl RuntimeEndpoint {
    pub fn unix_socket(path: impl Into<PathBuf>) -> Self {
        Self::UnixSocket(path.into())
    }

    pub fn unix_socket_path(&self) -> Result<&Path, RuntimePathError> {
        match self {
            Self::UnixSocket(path) => Ok(path.as_path()),
            Self::WindowsNamedPipe(_) => Err(RuntimePathError::UnsupportedEndpoint(
                "windows named pipe transport is not implemented",
            )),
        }
    }

    pub fn display_label(&self) -> String {
        match self {
            Self::UnixSocket(path) => path.display().to_string(),
            Self::WindowsNamedPipe(name) => name.clone(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RuntimePathError {
    #[error("failed to resolve current executable: {0}")]
    CurrentExecutable(#[source] std::io::Error),
    #[error("{0}")]
    UnsupportedEndpoint(&'static str),
}

pub fn event_log_path(data_dir: &Path) -> PathBuf {
    data_dir.join("events").join("runtime.jsonl")
}

pub fn shim_path_from_env() -> Result<PathBuf, RuntimePathError> {
    shim_path(std::env::current_exe)
}

pub fn shim_path<F>(current_exe: F) -> Result<PathBuf, RuntimePathError>
where
    F: FnOnce() -> std::io::Result<PathBuf>,
{
    current_exe().map_err(RuntimePathError::CurrentExecutable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_log_path_uses_lilo_data_events_file() {
        assert_eq!(
            event_log_path(Path::new("/tmp/lilo/data")),
            PathBuf::from("/tmp/lilo/data/events/runtime.jsonl")
        );
    }

    #[test]
    fn shim_path_uses_current_executable() {
        let shim = shim_path(|| Ok(PathBuf::from("/bin/lilo"))).expect("shim path");
        assert_eq!(shim, PathBuf::from("/bin/lilo"));
    }

    #[test]
    fn unix_socket_endpoint_reports_path() {
        let endpoint = RuntimeEndpoint::unix_socket("/tmp/lilod.sock");
        assert_eq!(
            endpoint.unix_socket_path().expect("socket path"),
            Path::new("/tmp/lilod.sock")
        );
        assert_eq!(endpoint.display_label(), "/tmp/lilod.sock");
    }

    #[test]
    fn windows_named_pipe_is_modeled_but_not_supported() {
        let endpoint = RuntimeEndpoint::WindowsNamedPipe(r"\\.\pipe\lilod".to_owned());
        assert_eq!(endpoint.display_label(), r"\\.\pipe\lilod");
        assert!(matches!(
            endpoint.unix_socket_path(),
            Err(RuntimePathError::UnsupportedEndpoint(_))
        ));
    }
}
