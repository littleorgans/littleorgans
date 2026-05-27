pub const SESSIONS_SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY NOT NULL,
    runtime TEXT NOT NULL,
    role TEXT NOT NULL,
    workspace TEXT NOT NULL,
    namespace TEXT NOT NULL DEFAULT 'default',
    dir TEXT NOT NULL,
    state TEXT NOT NULL,
    lost_evidence TEXT,
    runtime_pid INTEGER NOT NULL,
    runtime_session TEXT,
    transcript_path TEXT,
    tmux_pane TEXT,
    agent_config TEXT,
    created_at TEXT NOT NULL,
    started_at TEXT NOT NULL,
    terminated_at TEXT,
    exit_code INTEGER,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS namespaces (
    slug TEXT PRIMARY KEY NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS mail (
    id TEXT PRIMARY KEY NOT NULL,
    sender_id TEXT NOT NULL,
    recipient_id TEXT NOT NULL,
    content TEXT NOT NULL,
    sent_at TEXT NOT NULL,
    read_at TEXT
);

CREATE INDEX IF NOT EXISTS idx_mail_recipient_unread
    ON mail(recipient_id, read_at, sent_at);

CREATE TABLE IF NOT EXISTS labels (
    session_id TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (session_id, key)
);

CREATE INDEX IF NOT EXISTS idx_labels_key_value_session
    ON labels(key, value, session_id);

CREATE TABLE IF NOT EXISTS event_cursor (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    cursor BLOB NOT NULL,
    updated_at TEXT NOT NULL
);
";
