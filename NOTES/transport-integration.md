# Transport Integration

**Status:** superseded as an implementation proposal. Retained as a routing
note.

The June 2026 proposal made the Transport Matters `tm` launcher the permanent
Little Organs launch wrapper and proposed migrating its Python API into the
monorepo. Current product direction supersedes that transplant model.

Transport Matters is now treated as a knowledge base, executable reference,
payload fixture library, and record of lessons. Little Organs owns its bounded
contexts and product surface. It may reuse proven algorithms and fixtures, but
it does not inherit Transport Matters package boundaries by default.

The complete prior proposal is preserved at
`.archive/transport-integration.v1.md`.

## Current Direction

The active architecture is documented in:

- [`docs/architecture/system.md`](../docs/architecture/system.md)
- [`docs/architecture/transport.md`](../docs/architecture/transport.md)
- [`docs/architecture/canvas.md`](../docs/architecture/canvas.md)
- [`docs/architecture/schedule.md`](../docs/architecture/schedule.md)

The central ownership rules are:

1. Session owns logical session intent and prepares capture.
2. Schedule is the target sole placement authority.
3. Runtime executes topology and process operations.
4. Transport owns provider wire capture, interpretation, transformation, and
   fidelity evidence.
5. Canvas and Desktop are one product surface.
6. Capture and transcript association remain between Transport and Session.
7. `SessionId` is the platform join key.
8. Schedule and Runtime receive no provider payload semantics.

## Decisions Carried Forward

The earlier note established useful decisions that remain valid:

- `lilo capture` is Runtime's tmux pane capture verb.
- Provider traffic capture is associated with session backed launches.
- Captured records correlate through the typed UUIDv4 `SessionId`.
- Transport exposes operator and product read models rather than spawn
  authority.
- Aggregate `lilo doctor` remains the health surface.

## Decisions Reopened

The following proposals are no longer locked:

- the `tm` wrapper as the permanent launch chain;
- migration of the Transport Matters Python package;
- a Rust contract crate paired with a Python launcher;
- a separate Transport SQLite index under `~/.lilo/capture/`;
- FastAPI as the Little Organs read surface;
- `lilo transport` shelling to an external `tm` process;
- the exact implementation language and process topology;
- the final failure policy when capture cannot prepare or persist.

These choices follow the bounded architecture and first vertical slice. They
must not be locked ahead of evidence from the current Little Organs source and
the Transport Matters fixtures.

## Required Safety

Any implementation must preserve:

- exact original bytes when a request is unchanged;
- provider valid serialization after a real transformation;
- unknown provider fields;
- Codex WebSocket and HTTPS fallback parity;
- safe `previous_response_id` continuation behavior;
- request scoped positional identities only;
- durable original, forwarded, response, and audit evidence.
