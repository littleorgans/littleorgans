#![cfg_attr(test, allow(clippy::expect_used, clippy::unwrap_used))]

//! User facing CLI and embedded MCP bridge for talking to rtmd.
//!
//! The binary builds typed client requests and delegates daemon state through
//! `lilo-rm-client`.

pub mod cli;
pub mod generated;
pub mod mcp;
pub mod shared;

pub const VERSION: &str = env!("RTM_CLI_VERSION");
