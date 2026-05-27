#![deny(unsafe_code)]

//! Local littleorgans path policy plus folded runtime and session path contracts.

mod env;
mod lilo;
mod runtime;
mod session;

pub use lilo::{DaemonEndpoint, LiloHome, LiloPathError, LiloPaths};
pub use runtime::{
    HOME, RTM_DB_PATH, RTM_HOME, RTM_SHIM_PATH, RTM_SOCKET_PATH, RuntimeEndpoint, RuntimePathEnv,
    RuntimePathError, XDG_RUNTIME_DIR, db_path, db_path_from_env, display_unix_socket_path,
    display_unix_socket_path_from_env, event_log_path, log_root, log_root_from_env,
    runtime_endpoint, runtime_endpoint_from_env, shim_path, shim_path_from_env, unix_socket_path,
    unix_socket_path_from_env,
};
pub use session::{
    SM_DB_PATH, SM_HOME, SM_LOG_PATH, SM_SOCKET_PATH, SmEndpoint, SmPaths, SmPathsEnv,
    SmPathsError, rtmd_socket_path, rtmd_socket_path_from,
};

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;

    #[test]
    fn session_surface_is_exported_from_crate_root() {
        let env = SmPathsEnv::new()
            .sm_home("/tmp/sm-home")
            .sm_socket_path("/tmp/sm.sock")
            .rtm_socket_path("/tmp/rtm.sock");

        let paths = SmPaths::resolve(&env).expect("paths resolve");
        let endpoint = SmEndpoint::resolve(&env).expect("endpoint resolves");

        assert_eq!(SM_HOME, "SM_HOME");
        assert_eq!(SM_DB_PATH, "SM_DB_PATH");
        assert_eq!(SM_LOG_PATH, "SM_LOG_PATH");
        assert_eq!(SM_SOCKET_PATH, "SM_SOCKET_PATH");
        assert_eq!(paths.dir, PathBuf::from("/tmp/sm-home"));
        assert_eq!(endpoint.as_path(), Path::new("/tmp/sm.sock"));
        assert_eq!(rtmd_socket_path_from(&env), PathBuf::from("/tmp/rtm.sock"));
        assert_eq!(SmPathsError::MissingHome, LiloPathError::MissingHome);
    }
}
