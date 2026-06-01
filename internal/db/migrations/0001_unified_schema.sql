CREATE TABLE identity_audit (
    id TEXT NOT NULL PRIMARY KEY,
    timestamp TEXT NOT NULL,
    principal TEXT NOT NULL,
    action TEXT NOT NULL,
    resource TEXT NOT NULL,
    decision TEXT NOT NULL,
    session_ref TEXT NULL,
    notes TEXT NULL,
    policy_id TEXT NULL,
    evaluation_trace TEXT NULL,
    denial_reason TEXT NULL
);

CREATE INDEX idx_identity_audit_timestamp ON identity_audit(timestamp);
CREATE INDEX idx_identity_audit_session_ref ON identity_audit(session_ref);

CREATE TABLE session_sessions (
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

CREATE INDEX idx_session_sessions_namespace_terminated
    ON session_sessions(namespace, terminated_at);

CREATE TABLE session_namespaces (
    slug TEXT PRIMARY KEY NOT NULL,
    created_at TEXT NOT NULL
);

INSERT INTO session_namespaces (slug, created_at)
VALUES ('default', strftime('%Y-%m-%dT%H:%M:%fZ', 'now'));

CREATE TABLE messages (
    message_id TEXT PRIMARY KEY NOT NULL,
    sender_ref TEXT NOT NULL,
    context_id TEXT NOT NULL,
    intent TEXT NOT NULL,
    idempotency_key TEXT,
    content TEXT NOT NULL,
    sent_at TEXT NOT NULL
);

CREATE UNIQUE INDEX idx_messages_sender_idempotency
    ON messages(sender_ref, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE INDEX idx_messages_context_sent
    ON messages(context_id, sent_at, message_id);

CREATE INDEX idx_messages_sender_sent
    ON messages(sender_ref, sent_at, message_id);

CREATE TABLE message_deliveries (
    message_id TEXT NOT NULL,
    recipient_session_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('unread', 'read')),
    read_at TEXT,
    PRIMARY KEY (message_id, recipient_session_id)
);

CREATE INDEX idx_message_deliveries_recipient_unread
    ON message_deliveries(recipient_session_id, status, read_at, message_id);

CREATE INDEX idx_message_deliveries_message
    ON message_deliveries(message_id);

CREATE TABLE session_labels (
    session_id TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (session_id, key)
);

CREATE INDEX idx_session_labels_key_value_session
    ON session_labels(key, value, session_id);

CREATE TABLE session_event_cursor (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    cursor BLOB NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE session_spawn_intents (
    session_id TEXT PRIMARY KEY,
    operation_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('pending', 'resolved', 'aborted')),
    spawn_request_json TEXT NOT NULL,
    session_draft_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    resolved_at INTEGER,
    aborted_reason TEXT
);

CREATE INDEX idx_session_spawn_intents_status_created
    ON session_spawn_intents(status, created_at);

CREATE TABLE runtime_lifecycle (
    session_id TEXT PRIMARY KEY NOT NULL,
    runtime TEXT NOT NULL,
    isolation TEXT NOT NULL DEFAULT 'host',
    state TEXT NOT NULL,
    shim_pid INTEGER,
    runtime_pid INTEGER,
    start_time TEXT,
    tmux_pane TEXT,
    exit_code INTEGER,
    exit_signal INTEGER,
    lost_evidence TEXT,
    spawned_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX idx_runtime_lifecycle_state ON runtime_lifecycle(state);
CREATE INDEX idx_runtime_lifecycle_spawned_at ON runtime_lifecycle(spawned_at);

CREATE TABLE runtime_metadata (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
