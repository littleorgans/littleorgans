use std::path::PathBuf;

use lilo_rm_core::{Lifecycle, LogAvailability};

pub use lilo_paths::{DaemonEndpoint, LiloPathError, LiloPaths};

#[must_use]
pub fn lifecycle_transcript_path(lifecycle: &Lifecycle) -> Option<PathBuf> {
    match lifecycle.log_availability.as_ref() {
        Some(LogAvailability::Headless { stdout_path, .. }) => Some(stdout_path.clone()),
        Some(LogAvailability::TmuxPaneSnapshot | LogAvailability::Unavailable { .. }) | None => {
            None
        }
    }
}
