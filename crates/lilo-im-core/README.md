# lilo-im-core

AuthZ is NOT enforced in identity-matters v1. This crate locks the shared IAM boundary for callers today, while the v2+ roadmap replaces `lilo-im-stub` with an enforcing `lilo-im-daemon` behind the same `Authorizer` contract.

`lilo-im-core` owns the stable authorization types:

- `Principal` identifies the caller. v1 supports `Local(uid)` and preserves unknown tagged variants so newer producers can write audit rows that older readers can still inspect.
- `Action` is closed in v1: `Spawn`, `Kill`, `List`, `Read`, `Logs`, `MailSend`, `MailRead`, `Nudge`, `Link`, `Doctor`, and `Daemon`. v2+ may extend the enum through a deliberate contract change.
- `ResourceSpec` describes the target of the request without binding identity-matters to a specific caller.
- `AuditRow` records every authorization decision, including nullable reserved fields that later policy engines can populate without changing the table shape.

The principal wire format uses a `kind` tag. `Local(uid)` serializes as:

```json
{
  "kind": "Local",
  "uid": 501
}
```

Unknown principal kinds deserialize into `Principal::Unknown` with their original fields preserved for audit inspection and reserialization.
