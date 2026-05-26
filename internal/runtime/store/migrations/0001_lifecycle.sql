CREATE TABLE lifecycle (
    session_id TEXT PRIMARY KEY NOT NULL,
    runtime TEXT NOT NULL,
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

CREATE INDEX lifecycle_state_idx ON lifecycle(state);
CREATE INDEX lifecycle_spawned_at_idx ON lifecycle(spawned_at);
