mod config;
mod events;
mod runner;
mod spawn;
mod state;
mod status;
mod termination;
mod watcher;

pub use config::DaemonConfig;
pub use runner::{run_daemon, run_daemon_with_db};

pub(crate) use state::ServerState;

#[cfg(test)]
use {
    crate::reconcile,
    lilo_rm_core::{
        CaptureError, CaptureRequest, CaptureResponse, KillRequest, Lifecycle, LogAvailability,
        LogsUnavailableReason, LostEvidence, NudgeFailureReason, NudgeOutcome, NudgeRequest,
        NudgeResponse, RuntimeExit, RuntimeSignal, ShimReady, StatusFilter,
    },
    lilo_runtime_store::{LifecycleStore, StoreConfig},
    std::path::PathBuf,
    uuid::Uuid,
};

#[cfg(test)]
mod tests;
