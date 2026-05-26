CREATE TABLE IF NOT EXISTS audit (
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

CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit(timestamp);
CREATE INDEX IF NOT EXISTS idx_audit_session_ref ON audit(session_ref);
