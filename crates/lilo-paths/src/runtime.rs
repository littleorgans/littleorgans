//! Runtime filesystem path policy and daemon endpoint modeling.
//!
//! Filesystem locations are plain `PathBuf` values. Daemon connection targets
//! are `RuntimeEndpoint` values so future transports do not masquerade as
//! filesystem paths.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

pub const RTM_SOCKET_PATH: &str = "RTM_SOCKET_PATH";
pub const RTM_DB_PATH: &str = "RTM_DB_PATH";
pub const RTM_HOME: &str = "RTM_HOME";
pub const RTM_SHIM_PATH: &str = "RTM_SHIM_PATH";
pub const XDG_RUNTIME_DIR: &str = "XDG_RUNTIME_DIR";
pub const HOME: &str = "HOME";

const SOCKET_FILE: &str = "sock";
const DB_FILE: &str = "db.sqlite";
const LOG_DIR: &str = "logs";
const EVENT_LOG_FILE: &str = "events.jsonl";

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

    pub fn display_label(&self, env: &RuntimePathEnv) -> String {
        match self {
            Self::UnixSocket(path) => display_unix_socket_path(path, env),
            Self::WindowsNamedPipe(name) => name.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimePathEnv {
    socket_path: Option<OsString>,
    db_path: Option<OsString>,
    rtm_home: Option<OsString>,
    shim_path: Option<OsString>,
    xdg_runtime_dir: Option<OsString>,
    home: Option<OsString>,
}

impl RuntimePathEnv {
    pub fn from_process() -> Self {
        Self {
            socket_path: std::env::var_os(RTM_SOCKET_PATH),
            db_path: std::env::var_os(RTM_DB_PATH),
            rtm_home: std::env::var_os(RTM_HOME),
            shim_path: std::env::var_os(RTM_SHIM_PATH),
            xdg_runtime_dir: std::env::var_os(XDG_RUNTIME_DIR),
            home: std::env::var_os(HOME),
        }
    }

    pub fn new() -> Self {
        Self {
            socket_path: None,
            db_path: None,
            rtm_home: None,
            shim_path: None,
            xdg_runtime_dir: None,
            home: None,
        }
    }

    #[must_use]
    pub fn socket_path(mut self, value: impl Into<OsString>) -> Self {
        self.socket_path = Some(value.into());
        self
    }

    #[must_use]
    pub fn db_path(mut self, value: impl Into<OsString>) -> Self {
        self.db_path = Some(value.into());
        self
    }

    #[must_use]
    pub fn rtm_home(mut self, value: impl Into<OsString>) -> Self {
        self.rtm_home = Some(value.into());
        self
    }

    #[must_use]
    pub fn shim_path(mut self, value: impl Into<OsString>) -> Self {
        self.shim_path = Some(value.into());
        self
    }

    #[must_use]
    pub fn xdg_runtime_dir(mut self, value: impl Into<OsString>) -> Self {
        self.xdg_runtime_dir = Some(value.into());
        self
    }

    #[must_use]
    pub fn home(mut self, value: impl Into<OsString>) -> Self {
        self.home = Some(value.into());
        self
    }
}

impl Default for RuntimePathEnv {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RuntimePathError {
    #[error("HOME is required for default {context}")]
    MissingHome { context: &'static str },
    #[error("failed to resolve current executable: {0}")]
    CurrentExecutable(#[source] std::io::Error),
    #[error("{0}")]
    UnsupportedEndpoint(&'static str),
}

pub fn runtime_endpoint_from_env() -> Result<RuntimeEndpoint, RuntimePathError> {
    runtime_endpoint(&RuntimePathEnv::from_process())
}

pub fn runtime_endpoint(env: &RuntimePathEnv) -> Result<RuntimeEndpoint, RuntimePathError> {
    Ok(RuntimeEndpoint::UnixSocket(unix_socket_path(env)?))
}

pub fn unix_socket_path_from_env() -> Result<PathBuf, RuntimePathError> {
    unix_socket_path(&RuntimePathEnv::from_process())
}

pub fn unix_socket_path(env: &RuntimePathEnv) -> Result<PathBuf, RuntimePathError> {
    if let Some(path) = env.socket_path.as_ref() {
        return Ok(PathBuf::from(path));
    }

    platform_default_socket_path(env)
}

pub fn display_unix_socket_path_from_env(path: &Path) -> String {
    display_unix_socket_path(path, &RuntimePathEnv::from_process())
}

pub fn display_unix_socket_path(path: &Path, env: &RuntimePathEnv) -> String {
    if env.socket_path.is_some() {
        return path.display().to_string();
    }

    let Ok(default_path) = unix_socket_path(env) else {
        return path.display().to_string();
    };
    if path == default_path {
        default_socket_path_label(env)
    } else {
        path.display().to_string()
    }
}

pub fn db_path_from_env() -> Result<PathBuf, RuntimePathError> {
    db_path(&RuntimePathEnv::from_process())
}

pub fn db_path(env: &RuntimePathEnv) -> Result<PathBuf, RuntimePathError> {
    if let Some(path) = env.db_path.as_ref() {
        return Ok(PathBuf::from(path));
    }

    Ok(home_runtime_dir(env, "rtm db path")?.join(DB_FILE))
}

pub fn log_root_from_env() -> Result<PathBuf, RuntimePathError> {
    log_root(&RuntimePathEnv::from_process())
}

pub fn log_root(env: &RuntimePathEnv) -> Result<PathBuf, RuntimePathError> {
    Ok(runtime_home(env, "rtm log path")?.join(LOG_DIR))
}

pub fn event_log_path(data_dir: &Path) -> PathBuf {
    data_dir.join(EVENT_LOG_FILE)
}

pub fn shim_path_from_env() -> Result<PathBuf, RuntimePathError> {
    shim_path(&RuntimePathEnv::from_process(), std::env::current_exe)
}

pub fn shim_path<F>(env: &RuntimePathEnv, current_exe: F) -> Result<PathBuf, RuntimePathError>
where
    F: FnOnce() -> std::io::Result<PathBuf>,
{
    if let Some(path) = env.shim_path.as_ref() {
        return Ok(PathBuf::from(path));
    }

    current_exe().map_err(RuntimePathError::CurrentExecutable)
}

fn runtime_home(env: &RuntimePathEnv, context: &'static str) -> Result<PathBuf, RuntimePathError> {
    if let Some(path) = non_empty_path(env.rtm_home.as_ref()) {
        return Ok(path);
    }

    home_runtime_dir(env, context)
}

fn home_runtime_dir(
    env: &RuntimePathEnv,
    context: &'static str,
) -> Result<PathBuf, RuntimePathError> {
    let home = env
        .home
        .as_ref()
        .ok_or(RuntimePathError::MissingHome { context })?;
    Ok(PathBuf::from(home).join(".rtm"))
}

#[cfg(target_os = "linux")]
fn platform_default_socket_path(env: &RuntimePathEnv) -> Result<PathBuf, RuntimePathError> {
    if let Some(runtime_dir) = non_empty_path(env.xdg_runtime_dir.as_ref()) {
        return Ok(runtime_dir.join("rtm").join(SOCKET_FILE));
    }

    Ok(home_runtime_dir(env, "rtm socket path")?.join(SOCKET_FILE))
}

#[cfg(not(target_os = "linux"))]
fn platform_default_socket_path(env: &RuntimePathEnv) -> Result<PathBuf, RuntimePathError> {
    Ok(home_runtime_dir(env, "rtm socket path")?.join(SOCKET_FILE))
}

fn non_empty_path(value: Option<&OsString>) -> Option<PathBuf> {
    value.filter(|value| !value.is_empty()).map(PathBuf::from)
}

fn default_socket_path_label(env: &RuntimePathEnv) -> String {
    platform_default_socket_path_label(env)
}

#[cfg(target_os = "linux")]
fn platform_default_socket_path_label(env: &RuntimePathEnv) -> String {
    if non_empty_path(env.xdg_runtime_dir.as_ref()).is_some() {
        return "$XDG_RUNTIME_DIR/rtm/sock".to_owned();
    }

    "~/.rtm/sock".to_owned()
}

#[cfg(not(target_os = "linux"))]
fn platform_default_socket_path_label(_env: &RuntimePathEnv) -> String {
    "~/.rtm/sock".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_socket_path_wins() {
        let env = RuntimePathEnv::new()
            .socket_path("/tmp/custom.sock")
            .xdg_runtime_dir("/run/user/1000")
            .home("/home/alice");

        let endpoint = runtime_endpoint(&env).expect("endpoint");

        assert_eq!(
            endpoint,
            RuntimeEndpoint::UnixSocket(PathBuf::from("/tmp/custom.sock"))
        );
        assert_eq!(endpoint.display_label(&env), "/tmp/custom.sock");
    }

    #[test]
    fn explicit_db_and_shim_paths_win_even_when_empty() {
        let env = RuntimePathEnv::new().db_path("").shim_path("");

        let shim = shim_path(&env, || Ok(PathBuf::from("/bin/rtm"))).expect("shim path");

        assert_eq!(db_path(&env).expect("db path"), PathBuf::from(""));
        assert_eq!(shim, PathBuf::from(""));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_uses_xdg_runtime_dir_when_available() {
        let env = RuntimePathEnv::new()
            .xdg_runtime_dir("/run/user/1000")
            .home("/home/alice");

        let endpoint = runtime_endpoint(&env).expect("endpoint");

        assert_eq!(
            endpoint,
            RuntimeEndpoint::UnixSocket(PathBuf::from("/run/user/1000/rtm/sock"))
        );
        assert_eq!(endpoint.display_label(&env), "$XDG_RUNTIME_DIR/rtm/sock");
    }

    #[test]
    fn falls_back_to_home_runtime_dir() {
        let env = RuntimePathEnv::new().home("/home/alice");

        assert_eq!(
            unix_socket_path(&env).expect("socket path"),
            PathBuf::from("/home/alice/.rtm/sock")
        );
        assert_eq!(
            db_path(&env).expect("db path"),
            PathBuf::from("/home/alice/.rtm/db.sqlite")
        );
        assert_eq!(
            log_root(&env).expect("log root"),
            PathBuf::from("/home/alice/.rtm/logs")
        );
    }

    #[test]
    fn rtm_home_only_changes_log_root() {
        let env = RuntimePathEnv::new()
            .rtm_home("/tmp/rtm-home")
            .home("/home/alice");

        assert_eq!(
            unix_socket_path(&env).expect("socket path"),
            PathBuf::from("/home/alice/.rtm/sock")
        );
        assert_eq!(
            db_path(&env).expect("db path"),
            PathBuf::from("/home/alice/.rtm/db.sqlite")
        );
        assert_eq!(
            log_root(&env).expect("log root"),
            PathBuf::from("/tmp/rtm-home/logs")
        );
    }

    #[test]
    fn empty_rtm_home_and_xdg_runtime_dir_are_ignored() {
        let env = RuntimePathEnv::new()
            .rtm_home("")
            .xdg_runtime_dir("")
            .home("/home/alice");

        assert_eq!(
            unix_socket_path(&env).expect("socket path"),
            PathBuf::from("/home/alice/.rtm/sock")
        );
        assert_eq!(
            log_root(&env).expect("log root"),
            PathBuf::from("/home/alice/.rtm/logs")
        );
    }

    #[test]
    fn event_log_path_is_under_data_dir() {
        assert_eq!(
            event_log_path(Path::new("/tmp/rtm")),
            PathBuf::from("/tmp/rtm/events.jsonl")
        );
    }

    #[test]
    fn windows_named_pipe_has_display_label_and_no_unix_path() {
        let endpoint = RuntimeEndpoint::WindowsNamedPipe(r"\\.\pipe\rtm".to_owned());

        assert_eq!(
            endpoint.display_label(&RuntimePathEnv::new()),
            r"\\.\pipe\rtm"
        );
        assert!(matches!(
            endpoint.unix_socket_path(),
            Err(RuntimePathError::UnsupportedEndpoint(_))
        ));
    }
}
