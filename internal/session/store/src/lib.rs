#![forbid(unsafe_code)]

pub mod postgres;

#[cfg(test)]
#[path = "../../test_support.rs"]
mod test_support;

pub use postgres::{
    MailWriteOutcome, PendingSpawnIntent, SessionDraft, SessionSpawnIntent, SessionStore,
    SpawnIntentError, SpawnIntentStatus,
};
