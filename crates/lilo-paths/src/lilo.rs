use std::fmt;
use std::path::{Path, PathBuf};

use crate::env::env_path;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub(crate) const LILO_HOME_ENV: &str = "LILO_HOME";
pub(crate) const LILO_SOCKET_PATH_ENV: &str = "LILO_SOCKET_PATH";
const HOME_ENV: &str = "HOME";

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LiloHome {
    root: PathBuf,
}

impl LiloHome {
    pub fn from_env() -> Result<Self, LiloPathError> {
        if let Some(path) = env_path(LILO_HOME_ENV) {
            return Self::from_path(path);
        }

        let home = env_path(HOME_ENV).ok_or(LiloPathError::MissingHome)?;
        Self::from_path(home.join(".lilo"))
    }

    pub fn from_path(path: PathBuf) -> Result<Self, LiloPathError> {
        if path.as_os_str().is_empty() {
            return Err(LiloPathError::EmptyPath);
        }

        Ok(Self { root: path })
    }

    fn join(&self, path: impl AsRef<Path>) -> PathBuf {
        self.root.join(path)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct LiloPaths {
    home: LiloHome,
}

impl LiloPaths {
    pub fn new(home: LiloHome) -> Self {
        Self { home }
    }

    pub fn config_root(&self) -> PathBuf {
        self.home.join("config")
    }

    pub fn run_root(&self) -> PathBuf {
        self.home.join("run")
    }

    pub fn data_root(&self) -> PathBuf {
        self.home.join("data")
    }

    pub fn logs_root(&self) -> PathBuf {
        self.home.join("logs")
    }

    pub fn agent_config_dir(&self, name: impl fmt::Display) -> PathBuf {
        self.config_root()
            .join("session")
            .join("agents")
            .join(name.to_string())
    }

    pub fn namespace_binding(&self) -> PathBuf {
        self.config_root().join("session").join("namespace")
    }

    pub fn session_log(&self, id: impl fmt::Display) -> PathBuf {
        self.logs_root().join("sessions").join(format!("{id}.log"))
    }

    pub fn runtime_log_dir(&self, id: impl fmt::Display) -> PathBuf {
        self.logs_root().join("runtimes").join(id.to_string())
    }

    pub fn lilod_log(&self) -> PathBuf {
        self.logs_root().join("lilod.log")
    }

    pub fn cache_root(&self) -> PathBuf {
        self.home.join("cache")
    }

    pub fn tmp_root(&self) -> PathBuf {
        self.home.join("tmp")
    }

    pub fn socket_path(&self) -> PathBuf {
        env_path(LILO_SOCKET_PATH_ENV).unwrap_or_else(|| self.run_root().join("lilod.sock"))
    }

    pub fn pid_path(&self) -> PathBuf {
        self.run_root().join("lilod.pid")
    }

    pub fn db_path(&self) -> PathBuf {
        self.data_root().join("lilo.db")
    }

    pub fn events_log_path(&self) -> PathBuf {
        self.data_root().join("events").join("runtime.jsonl")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DaemonEndpoint {
    socket_path: PathBuf,
}

impl DaemonEndpoint {
    pub fn from_paths(paths: &LiloPaths) -> Self {
        Self {
            socket_path: paths.socket_path(),
        }
    }

    pub fn as_path(&self) -> &Path {
        &self.socket_path
    }
}

impl fmt::Display for DaemonEndpoint {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.socket_path.display())
    }
}

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub enum LiloPathError {
    #[error("home directory is not available; set LILO_HOME or HOME")]
    MissingHome,
    #[error("path is empty")]
    EmptyPath,
}

#[cfg(test)]
#[allow(unsafe_code)]
mod tests {
    use super::*;
    use std::env;
    use std::ffi::OsString;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    const DEFAULT_HOME: &str = "/tmp/lilo-home";
    const FALLBACK_HOME: &str = "/tmp/home";
    const LEGACY_VALUE: &str = "/tmp/legacy";

    macro_rules! legacy_env_ignored_test {
        ($test_name:ident, $legacy_name:literal) => {
            #[test]
            fn $test_name() {
                assert_legacy_env_is_ignored($legacy_name);
            }
        };
    }

    #[test]
    fn lilo_home_env_roots_every_path() {
        let _env = EnvGuard::new(&[
            ("LILO_HOME", Some(DEFAULT_HOME)),
            ("LILO_SOCKET_PATH", None),
            ("HOME", Some("/tmp/ignored-home")),
        ]);

        let paths = paths_from_env();
        assert_default_tree(&paths, Path::new(DEFAULT_HOME));
    }

    #[test]
    fn home_env_fallback_roots_every_path_under_dot_lilo() {
        let _env = EnvGuard::new(&[
            ("LILO_HOME", None),
            ("LILO_SOCKET_PATH", None),
            ("HOME", Some(FALLBACK_HOME)),
        ]);

        let paths = paths_from_env();
        assert_default_tree(&paths, Path::new(FALLBACK_HOME).join(".lilo").as_path());
    }

    #[test]
    fn socket_override_wins_without_moving_other_paths() {
        let _env = EnvGuard::new(&[
            ("LILO_HOME", Some(DEFAULT_HOME)),
            ("LILO_SOCKET_PATH", Some("/custom/sock")),
            ("HOME", Some("/tmp/ignored-home")),
        ]);

        let paths = paths_from_env();
        assert_eq!(paths.socket_path(), PathBuf::from("/custom/sock"));
        assert_eq!(paths.run_root(), PathBuf::from(DEFAULT_HOME).join("run"));
        assert_eq!(
            paths.pid_path(),
            PathBuf::from(DEFAULT_HOME).join("run/lilod.pid")
        );
        assert_eq!(
            paths.db_path(),
            PathBuf::from(DEFAULT_HOME).join("data/lilo.db")
        );
    }

    #[test]
    fn endpoint_uses_paths_display_and_stable_json_string() {
        let _env = EnvGuard::new(&[
            ("LILO_HOME", Some(DEFAULT_HOME)),
            ("LILO_SOCKET_PATH", Some("/custom/sock")),
            ("HOME", Some("/tmp/ignored-home")),
        ]);

        let paths = paths_from_env();
        let endpoint = DaemonEndpoint::from_paths(&paths);

        assert_eq!(endpoint.as_path(), Path::new("/custom/sock"));
        assert_eq!(endpoint.to_string(), "/custom/sock");

        let encoded = serde_json::to_string(&endpoint).expect("endpoint serializes");
        assert_eq!(encoded, "\"/custom/sock\"");
        let decoded: DaemonEndpoint =
            serde_json::from_str(&encoded).expect("endpoint deserializes");
        assert_eq!(decoded, endpoint);
    }

    #[test]
    fn missing_home_errors_when_no_lilo_home_or_home_exist() {
        let _env = EnvGuard::new(&[("LILO_HOME", None), ("HOME", None)]);

        assert_eq!(LiloHome::from_env(), Err(LiloPathError::MissingHome));
    }

    #[test]
    fn empty_explicit_path_errors() {
        assert_eq!(
            LiloHome::from_path(PathBuf::new()),
            Err(LiloPathError::EmptyPath)
        );
    }

    legacy_env_ignored_test!(rtm_home_is_ignored, "RTM_HOME");
    legacy_env_ignored_test!(rtm_socket_path_is_ignored, "RTM_SOCKET_PATH");
    legacy_env_ignored_test!(sm_home_is_ignored, "SM_HOME");
    legacy_env_ignored_test!(sm_socket_path_is_ignored, "SM_SOCKET_PATH");
    legacy_env_ignored_test!(sm_db_path_is_ignored, "SM_DB_PATH");
    legacy_env_ignored_test!(sm_namespace_is_ignored, "SM_NAMESPACE");
    legacy_env_ignored_test!(rtm_db_path_is_ignored, "RTM_DB_PATH");
    legacy_env_ignored_test!(lilo_db_path_is_ignored, "LILO_DB_PATH");
    legacy_env_ignored_test!(agm_home_is_ignored, "AGM_HOME");

    fn assert_legacy_env_is_ignored(legacy_name: &'static str) {
        let _env = EnvGuard::new(&[
            ("LILO_HOME", None),
            ("LILO_SOCKET_PATH", None),
            ("HOME", Some(FALLBACK_HOME)),
            (legacy_name, Some(LEGACY_VALUE)),
        ]);

        let root = Path::new(FALLBACK_HOME).join(".lilo");
        let paths = paths_from_env();
        assert_default_tree(&paths, root.as_path());
    }

    fn paths_from_env() -> LiloPaths {
        let home = LiloHome::from_env().expect("home resolves");
        LiloPaths::new(home)
    }

    fn assert_default_tree(paths: &LiloPaths, root: &Path) {
        assert_eq!(paths.config_root(), root.join("config"));
        assert_eq!(paths.run_root(), root.join("run"));
        assert_eq!(paths.data_root(), root.join("data"));
        assert_eq!(paths.logs_root(), root.join("logs"));
        assert_eq!(
            paths.agent_config_dir("demo"),
            root.join("config/session/agents/demo")
        );
        assert_eq!(
            paths.namespace_binding(),
            root.join("config/session/namespace")
        );
        assert_eq!(
            paths.session_log("019e6900"),
            root.join("logs/sessions/019e6900.log")
        );
        assert_eq!(
            paths.runtime_log_dir("019e6900"),
            root.join("logs/runtimes/019e6900")
        );
        assert_eq!(paths.lilod_log(), root.join("logs/lilod.log"));
        assert_eq!(paths.cache_root(), root.join("cache"));
        assert_eq!(paths.tmp_root(), root.join("tmp"));
        assert_eq!(paths.socket_path(), root.join("run/lilod.sock"));
        assert_eq!(paths.pid_path(), root.join("run/lilod.pid"));
        assert_eq!(paths.db_path(), root.join("data/lilo.db"));
        assert_eq!(
            paths.events_log_path(),
            root.join("data/events/runtime.jsonl")
        );
    }

    struct EnvGuard {
        _lock: MutexGuard<'static, ()>,
        originals: Vec<(&'static str, Option<OsString>)>,
    }

    impl EnvGuard {
        fn new(vars: &[(&'static str, Option<&'static str>)]) -> Self {
            let lock = ENV_LOCK
                .get_or_init(|| Mutex::new(()))
                .lock()
                .expect("env lock is available");
            let originals = vars
                .iter()
                .map(|(name, _)| (*name, env::var_os(name)))
                .collect();

            for (name, value) in vars {
                set_env(name, value.map(std::ffi::OsStr::new));
            }

            Self {
                _lock: lock,
                originals,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (name, value) in &self.originals {
                set_env(name, value.as_ref().map(OsString::as_os_str));
            }
        }
    }

    fn set_env(name: &str, value: Option<&std::ffi::OsStr>) {
        match value {
            Some(value) => {
                // SAFETY: These tests serialize all environment mutations through ENV_LOCK
                // and do not spawn threads that read the process environment.
                unsafe { env::set_var(name, value) };
            }
            None => {
                // SAFETY: These tests serialize all environment mutations through ENV_LOCK
                // and do not spawn threads that read the process environment.
                unsafe { env::remove_var(name) };
            }
        }
    }
}
