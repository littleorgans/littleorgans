# Transport Architecture

**Status:** bounded design direction. No Transport implementation exists in
the monorepo yet.

Transport owns the wire between an agent harness and its model provider. It
captures exact traffic, interprets provider payloads, applies explicitly
authorized request scoped transformations, and proves what was forwarded.

Transport Matters is the knowledge base, executable reference, payload fixture
library, and record of lessons for this context. Its package boundaries and
process topology are not the target Little Organs architecture.

## Ownership

Transport owns:

1. Provider wire interposition.
2. Exact raw request and response bytes.
3. Provider adapters and immutable normalized request and response models.
4. Opaque preservation of provider fields it does not interpret.
5. Request fingerprints and request scoped field references.
6. Overlay validation, transformation ordering, and audit evidence.
7. Provider valid serialization after a real change.
8. Fidelity evidence between original, interpreted, curated, and forwarded
   payloads.
9. Safe handling of Claude Messages and Codex Responses traffic.

Transport does not authorize, choose an agent, select a target, own Session
intent, reconcile placement, or execute a process.

## Launch Boundary

Session asks Transport to prepare capture for a typed `SessionId`. Transport
returns an opaque capture lease and launch additions. Session includes them in
the launch payload supplied to the current Runtime path or the target Schedule
path.

Schedule treats the entire launch payload as opaque. Runtime applies the
authoritative launch specification and starts the harness. Neither context
interprets payload policy.

The implementation language remains open. A Little Organs owned Rust service
or a Little Organs owned Python helper may satisfy the Transport port. The
implementation must not import or shell to Transport Matters as its permanent
architecture.

## Capture Record

A captured turn needs sufficient evidence to reproduce and judge forwarding:

```text
session id
request id
turn id
provider and transport metadata
exact request bytes
interpreted request model
optional curated request bytes
optional curated request model
transformation audit
exact response bytes
interpreted response model
timestamps and provenance
```

The exact storage schema and raw retention policy remain open. The platform
join is `SessionId`; provider conversation ids and native resume references are
additional evidence rather than replacement identities.

The prior decision places the transcript service between Transport and Session.
Transport owns capture and interpretation. Session owns the product association
and exposes the joined read model. The exact Postgres table ownership must be
locked before implementation.

## Forwarding Invariants

1. If the interpreted request is unchanged, forward the original bytes exactly.
2. Serialize only after an authorized transformation changes the request.
3. Preserve unknown provider fields through interpretation and serialization.
4. Validate a changed request with the provider adapter before forwarding.
5. Record original, curated, forwarded, and audit evidence.
6. Failures never produce a partially transformed request.
7. Capture failure behavior is explicit and visible to Session.

## Overlay Identity

Durable overlays use semantic identities. A tool description can use a tool
name. Future stable targets require provider and harness evidence that survives
ordering changes.

Array position, message index, block index, and JSON path position may identify
a field only inside one held request. A request scoped positional edit requires:

- an exact request fingerprint;
- the original field value or digest;
- the provider and request kind;
- explicit mismatch behavior.

The position cannot be stored as a reusable overlay identity.

## Continuation and Transport Safety

Codex Responses may use WebSocket delivery, HTTPS fallback, and
`previous_response_id` continuation. Those paths must retain identical payload
semantics.

When `previous_response_id` is present, Transport must not replay positional
content edits whose referenced content may be absent from the continuation.
Semantic transformations such as a tool description selected by tool name may
remain eligible when their preconditions still hold.

Regression fixtures must cover:

- WebSocket request and response traffic;
- HTTPS fallback;
- interrupted turns and connection close;
- tool result only continuations;
- `previous_response_id` continuations;
- exact byte forwarding for unchanged requests;
- unknown provider field preservation.

## Evidence Carried from Transport Matters

Carry forward behavior and fixtures from Transport Matters source at
`1d5c9b72`:

- Claude Messages and Codex HTTP fallback turn fixtures;
- subagent and continuation fixtures;
- immutable request and response IR;
- raw field preservation in provider adapters;
- exact byte pass through when unchanged;
- transformation ordering and audit ledger behavior;
- durable file write and recovery algorithms;
- HTML report rendering and escaping behavior.

Treat these structures as reference only:

- the standalone `tm` launcher chain;
- Python and FastAPI package boundaries;
- the process global override store;
- separate Canvas and Inspector products;
- spaces, runtime templates, and duplicate session models;
- the full breakpoint system and Activity layer;
- the Transport Matters storage root and identifiers;
- registry, signing, accepted cache, and refresh infrastructure.

## First Slice

The proposed first slice holds one Claude Messages request, renders it in
Canvas, permits one tool description edit selected by tool name, validates the
curated request, forwards it, and displays the response and audit evidence.

Codex may remain pass through for that slice. Its WebSocket, HTTPS fallback,
and continuation guards are required from the start because Transport becomes
part of the launch path.

The blocking interaction model, failure posture, redaction policy, storage
shape, and implementation language remain open product or architecture
decisions. See [system architecture](system.md).
