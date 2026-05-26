# lilo-im-store

AuthZ is NOT enforced in identity-matters v1. This crate owns identity-matters audit storage, including the reserved audit schema fields `policy_id`, `evaluation_trace`, and `denial_reason` that v2+ policy evaluation can populate without a migration.

`lilo-im-store` keeps IAM audit data inside identity-matters. Consumers query it through `SqliteAuditSink` and `query_audit` rather than hosting identity data in their own stores.

Reserved fields:

- `policy_id` is nullable in v1 because no policy engine selects a policy yet.
- `evaluation_trace` is nullable in v1 because the stub has no policy evaluation steps to record.
- `denial_reason` is nullable for allow and error rows, and populated for deny rows when a reason exists.
