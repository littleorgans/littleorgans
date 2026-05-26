# lilo-rm-client

Public client crate for talking to the Runtime Matters daemon over its Unix socket.

This package owns the v0.2 client side transport contract:

- connect to the rtmd Unix socket
- send `RuntimeRpc` values as newline delimited JSON
- receive `RuntimeResponse` values
- normalize connection, framing, and daemon error responses into typed errors

Protocol types live in `lilo-rm-core`. Process execution remains delegated to rtmd.
