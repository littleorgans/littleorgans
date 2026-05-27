#![forbid(unsafe_code)]

pub mod agent_config;
pub mod events;
pub mod handler;
pub mod identity_client;
pub mod lifecycle;
pub mod mcp_bridge;
#[doc(hidden)]
pub mod mcp_tools;
mod namespace;
pub mod polish;
pub mod reconcile;
pub mod server;
mod service;
pub mod socket;
mod spawn_request;
mod store_lock;

#[cfg(test)]
#[path = "../../test_support.rs"]
mod test_support;

pub use server::run_daemon;
pub use service::{SessionService, SessionServiceContext};
pub use socket::send_request;
