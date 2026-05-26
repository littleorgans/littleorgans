//! Session filesystem path policy and daemon endpoint modeling.

use std::env;
use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::LiloPathError;
use crate::env::non_empty_path;

pub use crate::runtime::{HOME, RTM_SOCKET_PATH, XDG_RUNTIME_DIR};

pub const SM_HOME: &str = "SM_HOME";
pub const SM_DB_PATH: &str = "SM_DB_PATH";
pub const SM_LOG_PATH: &str = "SM_LOG_PATH";
pub const SM_SOCKET_PATH: &str = "SM_SOCKET_PATH";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmPaths {
    pub dir: PathBuf,
    pub pidfile: PathBuf,
    pub database: PathBuf,
    pub log: PathBuf,
}

impl SmPaths {
    pub fn from_env() -> Result<Self, SmPathsError> {
        Self::resolve(&SmPathsEnv::from_process())
    }

    pub fn resolve(env: &SmPathsEnv) -> Result<Self, SmPathsError> {
        let dir = sm_home_dir(env)?;
        Ok(Self {
            pidfile: dir.join("sm.pid"),
            database: non_empty_path(env.sm_db_path.as_ref()).unwrap_or_else(|| dir.join("sm.db")),
            log: non_empty_path(env.sm_log_path.as_ref()).unwrap_or_else(|| dir.join("smd.log")),
            dir,
        })
    }

    #[must_use]
    pub fn namespace_binding(&self) -> PathBuf {
        self.dir.join("namespace")
    }

    #[must_use]
    pub fn new(dir: PathBuf) -> Self {
        Self {
            pidfile: dir.join("sm.pid"),
            database: dir.join("sm.db"),
            log: dir.join("smd.log"),
            dir,
        }
    }
}

#[non_exhaustive]
#[derive(Clone, PartialEq, Eq)]
pub enum SmEndpoint {
    UnixSocket(PathBuf),
}

impl SmEndpoint {
    pub fn from_env() -> Result<Self, SmPathsError> {
        Self::resolve(&SmPathsEnv::from_process())
    }

    pub fn resolve(env: &SmPathsEnv) -> Result<Self, SmPathsError> {
        let dir = sm_home_dir(env)?;
        Ok(non_empty_path(env.sm_socket_path.as_ref())
            .map_or_else(|| Self::UnixSocket(dir.join("sock")), Self::UnixSocket))
    }

    pub fn unix_socket(path: impl Into<PathBuf>) -> Self {
        Self::UnixSocket(path.into())
    }

    #[must_use]
    pub fn as_path(&self) -> &Path {
        match self {
            Self::UnixSocket(path) => path,
        }
    }

    #[must_use]
    pub fn exists(&self) -> bool {
        self.as_path().exists()
    }
}

impl fmt::Debug for SmEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnixSocket(path) => formatter.debug_tuple("UnixSocket").field(path).finish(),
        }
    }
}

impl fmt::Display for SmEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.as_path().display())
    }
}

/// Captured view of the environment variables that drive session path resolution.
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct SmPathsEnv {
    sm_home: Option<OsString>,
    sm_db_path: Option<OsString>,
    sm_log_path: Option<OsString>,
    sm_socket_path: Option<OsString>,
    rtm_socket_path: Option<OsString>,
    xdg_runtime_dir: Option<OsString>,
    home: Option<OsString>,
}

impl SmPathsEnv {
    pub fn from_process() -> Self {
        Self {
            sm_home: env::var_os(SM_HOME),
            sm_db_path: env::var_os(SM_DB_PATH),
            sm_log_path: env::var_os(SM_LOG_PATH),
            sm_socket_path: env::var_os(SM_SOCKET_PATH),
            rtm_socket_path: env::var_os(RTM_SOCKET_PATH),
            xdg_runtime_dir: env::var_os(XDG_RUNTIME_DIR),
            home: env::var_os(HOME),
        }
    }

    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn sm_home(mut self, value: impl Into<OsString>) -> Self {
        self.sm_home = Some(value.into());
        self
    }

    #[must_use]
    pub fn sm_db_path(mut self, value: impl Into<OsString>) -> Self {
        self.sm_db_path = Some(value.into());
        self
    }

    #[must_use]
    pub fn sm_log_path(mut self, value: impl Into<OsString>) -> Self {
        self.sm_log_path = Some(value.into());
        self
    }

    #[must_use]
    pub fn sm_socket_path(mut self, value: impl Into<OsString>) -> Self {
        self.sm_socket_path = Some(value.into());
        self
    }

    #[must_use]
    pub fn rtm_socket_path(mut self, value: impl Into<OsString>) -> Self {
        self.rtm_socket_path = Some(value.into());
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

pub type SmPathsError = LiloPathError;

#[must_use]
pub fn rtmd_socket_path() -> PathBuf {
    rtmd_socket_path_from(&SmPathsEnv::from_process())
}

#[must_use]
pub fn rtmd_socket_path_from(env: &SmPathsEnv) -> PathBuf {
    non_empty_path(env.rtm_socket_path.as_ref())
        .or_else(|| {
            non_empty_path(env.xdg_runtime_dir.as_ref()).map(|dir| dir.join("rtm").join("sock"))
        })
        .or_else(|| non_empty_path(env.home.as_ref()).map(|home| home.join(".rtm").join("sock")))
        .unwrap_or_else(|| PathBuf::from(".rtm").join("sock"))
}

fn sm_home_dir(env: &SmPathsEnv) -> Result<PathBuf, SmPathsError> {
    non_empty_path(env.sm_home.as_ref())
        .or_else(|| non_empty_path(env.home.as_ref()).map(|home| home.join(".sm")))
        .ok_or(SmPathsError::MissingHome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_paths_preserve_home_defaults() {
        let env = SmPathsEnv::new().sm_home("/tmp/sm-home");
        let paths = SmPaths::resolve(&env).expect("paths resolve");
        assert_eq!(paths.dir, PathBuf::from("/tmp/sm-home"));
        assert_eq!(paths.pidfile, PathBuf::from("/tmp/sm-home/sm.pid"));
        assert_eq!(paths.database, PathBuf::from("/tmp/sm-home/sm.db"));
        assert_eq!(paths.log, PathBuf::from("/tmp/sm-home/smd.log"));
        assert_eq!(
            paths.namespace_binding(),
            PathBuf::from("/tmp/sm-home/namespace")
        );
    }

    #[test]
    fn session_paths_use_database_and_log_overrides() {
        let env = SmPathsEnv::new()
            .sm_home("/tmp/sm-home")
            .sm_db_path("/tmp/custom.db")
            .sm_log_path("/tmp/custom.log");
        let paths = SmPaths::resolve(&env).expect("paths resolve");
        assert_eq!(paths.database, PathBuf::from("/tmp/custom.db"));
        assert_eq!(paths.log, PathBuf::from("/tmp/custom.log"));
    }

    #[test]
    fn sm_endpoint_uses_socket_override() {
        let env = SmPathsEnv::new()
            .sm_home("/tmp/sm-home")
            .sm_socket_path("/tmp/sm.sock");
        let endpoint = SmEndpoint::resolve(&env).expect("endpoint resolves");
        assert_eq!(endpoint.as_path(), Path::new("/tmp/sm.sock"));
        assert_eq!(endpoint.to_string(), "/tmp/sm.sock");
        assert!(format!("{endpoint:?}").starts_with("UnixSocket"));
    }

    #[test]
    fn sm_endpoint_uses_home_socket_default() {
        let env = SmPathsEnv::new().home("/tmp/home");
        let endpoint = SmEndpoint::resolve(&env).expect("endpoint resolves");
        assert_eq!(endpoint.as_path(), Path::new("/tmp/home/.sm/sock"));
    }

    #[test]
    fn empty_env_values_are_ignored() {
        let env = SmPathsEnv::new()
            .sm_home("")
            .sm_socket_path("")
            .sm_db_path("")
            .sm_log_path("")
            .home("/tmp/home");
        let paths = SmPaths::resolve(&env).expect("paths resolve");
        let endpoint = SmEndpoint::resolve(&env).expect("endpoint resolves");
        assert_eq!(paths.dir, PathBuf::from("/tmp/home/.sm"));
        assert_eq!(paths.database, PathBuf::from("/tmp/home/.sm/sm.db"));
        assert_eq!(paths.log, PathBuf::from("/tmp/home/.sm/smd.log"));
        assert_eq!(endpoint.as_path(), Path::new("/tmp/home/.sm/sock"));
    }

    #[test]
    fn missing_home_errors() {
        let env = SmPathsEnv::new();
        assert_eq!(SmPaths::resolve(&env), Err(SmPathsError::MissingHome));
        assert_eq!(SmEndpoint::resolve(&env), Err(SmPathsError::MissingHome));
    }

    #[test]
    fn rtmd_socket_path_prefers_explicit_env() {
        let env = SmPathsEnv::new()
            .rtm_socket_path("/tmp/rtm.sock")
            .xdg_runtime_dir("/run/user/501")
            .home("/tmp/home");
        assert_eq!(rtmd_socket_path_from(&env), PathBuf::from("/tmp/rtm.sock"));
    }

    #[test]
    fn rtmd_socket_path_uses_xdg_runtime_dir_before_home() {
        let env = SmPathsEnv::new()
            .xdg_runtime_dir("/run/user/501")
            .home("/tmp/home");
        assert_eq!(
            rtmd_socket_path_from(&env),
            PathBuf::from("/run/user/501/rtm/sock")
        );
    }

    #[test]
    fn rtmd_socket_path_falls_back_to_home() {
        let env = SmPathsEnv::new().home("/tmp/home");
        assert_eq!(
            rtmd_socket_path_from(&env),
            PathBuf::from("/tmp/home/.rtm/sock")
        );
    }

    #[test]
    fn rtmd_socket_path_falls_back_to_relative_when_unset() {
        let env = SmPathsEnv::new();
        assert_eq!(
            rtmd_socket_path_from(&env),
            PathBuf::from(".rtm").join("sock")
        );
    }

    #[test]
    fn session_paths_error_displays_humanly() {
        assert_eq!(
            SmPathsError::MissingHome.to_string(),
            "home directory is not available; set LILO_HOME or HOME"
        );
    }
}
