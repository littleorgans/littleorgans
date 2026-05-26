# lilo-im-stub

AuthZ is NOT enforced in identity-matters v1. This crate implements the `lilo-im-core` `Authorizer` boundary as a local stub so callers depend on the same contract that the v2+ `lilo-im-daemon` roadmap will enforce.

`StubAuthorizer` allows the configured `Principal::Local(uid)` as role `admin` and records an audit row for each decision. Other principals receive `AuthzError::UnknownPrincipal` and a deny audit row.

Use this crate only to establish the call site, lock the `lilo-im-core` types, and write the audit log from day one.
