pub const AUDIT_TABLE: &str = "audit";
pub const SCHEMA_VERSION_TABLE: &str = "_schema_version";
pub const AUDIT_SCHEMA_VERSION: i64 = 1;
pub const RESERVED_AUDIT_COLUMNS: [&str; 3] = ["policy_id", "evaluation_trace", "denial_reason"];
pub const AUDIT_MIGRATION_SQL: &str = include_str!("../migrations/0001_audit.sql");
