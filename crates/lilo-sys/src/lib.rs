//! Published OS platform seam for lilo.
//!
//! Public modules expose runtime-neutral primitives. Target-specific code lives
//! below `sys/` so OS selection has one home.

pub mod error;
pub mod process;
pub mod process_exit;
pub mod signal;

mod sys;

pub use error::{Error, Result};
