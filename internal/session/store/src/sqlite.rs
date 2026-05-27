mod events;
mod labels;
mod mail;
mod migrations;
mod namespaces;
mod sessions;
mod time;

use std::path::Path;

use rusqlite::{Connection, Result};

use crate::schema::SESSIONS_SCHEMA;

pub use mail::MailRowError;
pub use namespaces::{NamespaceRecord, NamespaceRowError, SessionNamespace};
pub use sessions::SessionRowError;

pub struct SqliteStore {
    connection: Connection,
}

impl SqliteStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let connection = Connection::open(path)?;
        Self::from_connection(connection)
    }

    pub fn open_in_memory() -> Result<Self> {
        let connection = Connection::open_in_memory()?;
        Self::from_connection(connection)
    }

    fn from_connection(connection: Connection) -> Result<Self> {
        connection.execute_batch(SESSIONS_SCHEMA)?;
        migrations::run(&connection)?;
        Ok(Self { connection })
    }
}
