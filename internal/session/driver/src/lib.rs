#![forbid(unsafe_code)]

mod conv;
pub mod driver;
mod in_process;
mod port;
pub mod rtmd;

#[cfg(test)]
#[path = "../../test_support.rs"]
mod test_support;

pub use conv::{runtime_spawn_request, spawned_process};
pub use driver::{
    CaptureResult, ChildExit, LaunchEnv, NudgeResult, RuntimeError, RuntimeFault, SpawnLaunch,
    SpawnedProcess,
};
pub use in_process::InProcessRuntime;
pub use lilo_port::{OpaqueFault, PortError};
pub use port::{RuntimePort, wait_for_terminal};
pub use rtmd::RtmdDriver;
