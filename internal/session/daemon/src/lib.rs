#![forbid(unsafe_code)]

pub mod agent_config;
mod background_task;
pub mod events;
pub mod handler;
pub mod identity_client;
pub mod lifecycle;
mod mail_safety;
pub mod mcp_bridge;
#[doc(hidden)]
pub mod mcp_tools;
mod namespace;
pub mod polish;
pub mod reconcile;
mod service;
pub mod socket;
mod spawn_request;
mod store_lock;

#[cfg(test)]
#[path = "../../test_support.rs"]
mod test_support;

pub use service::{SessionService, SessionServiceContext};
pub use socket::{send_request, send_request_with_timeout};
