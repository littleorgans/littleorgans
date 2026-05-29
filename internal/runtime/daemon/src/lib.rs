#![forbid(unsafe_code)]
#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

//! Local rtmd process that owns runtime lifecycle state.
//!
//! The daemon handles sockets, request dispatch, persistence, Docker wrapping,
//! event delivery, spawn orchestration, and reconciliation.

mod api;
mod backend;
mod docker_argv;
mod docker_command;
mod docker_mount_plan;
pub mod docker_preflight;
mod docker_runtime;
mod doctor;
mod error;
pub mod event_channel;
mod event_log;
mod handler;
mod identity;
mod mcp_bridge;
mod reconcile;
mod runtime_kill;
pub mod server;
mod service;
pub mod shim_socket;
pub mod socket;
mod spawn_preflight;
#[cfg(test)]
mod test_support;
pub(crate) mod version;

pub use api::SpawnOutcome;
pub use reconcile::ReconcileConfig;
pub use server::{DaemonConfig, run_daemon};
pub use service::{RuntimeService, RuntimeServiceContext};
