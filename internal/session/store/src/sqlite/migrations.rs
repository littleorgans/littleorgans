use std::collections::HashSet;

use rusqlite::{Connection, Result, params};

type MigrationFn = fn(&Connection) -> Result<()>;

struct Migration {
    id: &'static str,
    apply: MigrationFn,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        id: "001_sessions_started_at",
        apply: add_started_at,
    },
    Migration {
        id: "002_sessions_termination",
        apply: add_termination_columns,
    },
    Migration {
        id: "003_sessions_runtime_link",
        apply: add_runtime_link_columns,
    },
    Migration {
        id: "004_sessions_tmux_and_agent_config",
        apply: add_tmux_and_agent_config_columns,
    },
    Migration {
        id: "005_event_cursor_and_lost_evidence",
        apply: add_event_cursor_and_lost_evidence,
    },
    Migration {
        id: "006_namespace_storage",
        apply: add_namespace_storage,
    },
];

pub fn run(connection: &Connection) -> Result<()> {
    connection.execute(
        "CREATE TABLE IF NOT EXISTS _migrations (
            id TEXT PRIMARY KEY NOT NULL,
            applied_at TEXT NOT NULL
        )",
        [],
    )?;
    for migration in MIGRATIONS {
        if migration_applied(connection, migration.id)? {
            continue;
        }
        (migration.apply)(connection)?;
        connection.execute(
            "INSERT INTO _migrations (id, applied_at) VALUES (?1, datetime('now'))",
            [migration.id],
        )?;
    }
    Ok(())
}

fn migration_applied(connection: &Connection, id: &str) -> Result<bool> {
    connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM _migrations WHERE id = ?1)",
        [id],
        |row| row.get(0),
    )
}

fn add_started_at(connection: &Connection) -> Result<()> {
    if add_column_if_missing(connection, "started_at", "TEXT")? {
        connection.execute(
            "UPDATE sessions SET started_at = created_at WHERE started_at IS NULL",
            [],
        )?;
    }
    Ok(())
}

fn add_termination_columns(connection: &Connection) -> Result<()> {
    add_column_if_missing(connection, "terminated_at", "TEXT")?;
    add_column_if_missing(connection, "exit_code", "INTEGER")?;
    Ok(())
}

fn add_runtime_link_columns(connection: &Connection) -> Result<()> {
    add_column_if_missing(connection, "runtime_session", "TEXT")?;
    add_column_if_missing(connection, "transcript_path", "TEXT")?;
    Ok(())
}

fn add_tmux_and_agent_config_columns(connection: &Connection) -> Result<()> {
    add_column_if_missing(connection, "tmux_pane", "TEXT")?;
    add_column_if_missing(connection, "agent_config", "TEXT")?;
    Ok(())
}

fn add_event_cursor_and_lost_evidence(connection: &Connection) -> Result<()> {
    add_column_if_missing(connection, "lost_evidence", "TEXT")?;
    connection.execute(
        "CREATE TABLE IF NOT EXISTS event_cursor (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            cursor BLOB NOT NULL,
            updated_at TEXT NOT NULL
        )",
        [],
    )?;
    Ok(())
}

fn add_namespace_storage(connection: &Connection) -> Result<()> {
    if add_column_if_missing(connection, "namespace", "TEXT NOT NULL DEFAULT 'default'")? {
        connection.execute(
            "UPDATE sessions SET namespace = 'default' WHERE namespace IS NULL",
            [],
        )?;
    }
    if add_column_if_missing(connection, "dir", "TEXT NOT NULL DEFAULT ''")? {
        connection.execute("UPDATE sessions SET dir = workspace WHERE dir = ''", [])?;
    }
    connection.execute(
        "CREATE TABLE IF NOT EXISTS namespaces (
            slug TEXT PRIMARY KEY NOT NULL,
            created_at TEXT NOT NULL
        )",
        [],
    )?;
    connection.execute(
        "INSERT OR IGNORE INTO namespaces (slug, created_at)
         VALUES ('default', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",
        [],
    )?;
    connection.execute(
        // Partition GC count guard lookups by namespace, then terminated state.
        "CREATE INDEX IF NOT EXISTS idx_sessions_namespace_terminated
            ON sessions(namespace, terminated_at)",
        [],
    )?;
    Ok(())
}

fn add_column_if_missing(
    connection: &Connection,
    name: &'static str,
    sql_type: &'static str,
) -> Result<bool> {
    if session_columns(connection)?.contains(name) {
        return Ok(false);
    }
    connection.execute(
        &format!("ALTER TABLE sessions ADD COLUMN {name} {sql_type}"),
        params![],
    )?;
    Ok(true)
}

fn session_columns(connection: &Connection) -> Result<HashSet<String>> {
    let mut statement = connection.prepare("PRAGMA table_info(sessions)")?;
    let rows = statement.query_map([], |row| row.get::<_, String>("name"))?;
    rows.collect()
}
