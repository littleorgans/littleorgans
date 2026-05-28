#![deny(unsafe_code)]

//! Local littleorgans path policy plus folded runtime and session path contracts.

mod env;
mod lilo;
mod runtime;

pub use lilo::{DaemonEndpoint, LiloHome, LiloPathError, LiloPaths, expand_home_path};
pub use runtime::{
    RuntimeEndpoint, RuntimePathError, event_log_path, shim_path, shim_path_from_env,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_surface_exports_lilo_policy() {
        let home = LiloHome::from_path("/tmp/lilo".into()).expect("home");
        let paths = LiloPaths::new(home);
        let endpoint = DaemonEndpoint::from_paths(&paths);

        assert_eq!(endpoint.as_path(), paths.socket_path());
    }
}
