//! Local littleorgans path policy plus folded runtime path contracts.

mod lilo;
mod runtime;

pub use lilo::{DaemonEndpoint, LiloHome, LiloPathError, LiloPaths};
pub use runtime::{
    HOME, RTM_DB_PATH, RTM_HOME, RTM_SHIM_PATH, RTM_SOCKET_PATH, RuntimeEndpoint, RuntimePathEnv,
    RuntimePathError, XDG_RUNTIME_DIR, db_path, db_path_from_env, display_unix_socket_path,
    display_unix_socket_path_from_env, event_log_path, log_root, log_root_from_env,
    runtime_endpoint, runtime_endpoint_from_env, shim_path, shim_path_from_env, unix_socket_path,
    unix_socket_path_from_env,
};
