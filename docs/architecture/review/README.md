# Architecture review and convergence proposal

- Status: Discussion draft
- Reviewed commit: `753e2ee91e3d41ae74dbe1cae3ec3ee6797e61da`
- Review date: 2026-08-14
- Scope: v0.8.0 architecture and readiness for the first Transport and Canvas proof

This page is the discussion landing for the architecture review. It separates accepted evidence from proposed changes. No proposal on this page is a locked decision.

## Review judgment

The target architecture is coherent. The implementation has a sound v1 base: one `lilo` command, one `lilod` process, one Postgres database, typed identifiers, and explicit Identity, Session, and Runtime contexts.

Three structural issues should be resolved before the first Transport and Canvas proof:

1. The public package graph depends on private packages.
2. Session reaches through the Runtime boundary into Runtime storage and services.
3. The documented opaque launch payload has no implemented type or handoff point.

Two cleanup tasks should travel with that work. The repository still exposes legacy daemon topology, and typed identifiers become strings inside the in-process Session to Runtime path.

## Current implementation

The live composition root is `internal/session/app/src/compose.rs`. It opens the shared database, constructs Runtime and Session services, reconciles unfinished intent, and starts one socket listener.

The normal session backed launch follows this sequence:

```text
lilo CLI
   |
   v
lilod socket
   |
   v
Session handler
   |
   +--> transaction A
   |      authorization
   |      pending spawn intent
   |      Runtime Forking state
   |
   +--> RuntimePort
   |      |
   |      v
   |   InProcessRuntime
   |      |
   |      v
   |   RuntimeService
   |      |
   |      v
   |   shim, tmux, child process
   |
   +--> transaction B
          session record
          Runtime Running state
          resolved intent
          lifecycle event
```

The two transaction intent protocol is worth preserving. Session commits authorization, pending intent, and Runtime Forking evidence before process creation. It commits the Session record, Runtime Running evidence, and intent resolution after Runtime reports readiness.

## Architecture constraints

The proposal preserves these project decisions:

1. Session owns user intent and user verbs.
2. Runtime owns process execution and raw runtime status.
3. Schedule becomes the sole placement authority when it activates.
4. Transport owns provider traffic and authorized request transformation.
5. Canvas reads and commands Session and Transport through `lilod`.
6. The v0.8.0 work does not activate Schedule.
7. One Postgres database may support atomic work across contexts. A shared database does not transfer ownership between contexts.
8. The typed UUIDv4 family remains the join key.

## Proposal 1: remove the old process topology

The current process story should have one production entrypoint. `compose.rs` already provides it.

Delete or internalize these old paths:

1. The alternate Session accept loop in `internal/session/daemon/src/server.rs`.
2. `SessionServiceContext::from_env` when no production caller needs it.
3. The separate `sm` and `rtm` distribution metadata.
4. Architecture prose that still describes the separate daemons as current or pending migration work.

Keep separate wire behavior only where tests need it for contract comparison.

Acceptance criteria:

1. Production startup has one composition root and one socket accept loop.
2. No distributable `sm` or `rtm` binary remains.
3. The direct Runtime API and the Runtime wire adapter still pass the same conformance tests.
4. `just check && just build && just test` passes.

## Proposal 2: repair the package publication graph

The public `lilo-rm-client` package depends on private `lilo-wire`. `lilo-wire` also depends on the private Session protocol. The publishable `lilo` package depends on several private application packages.

Use this ownership rule:

1. Runtime request and response contracts belong to Runtime.
2. The composed `LilodRpc` envelope stays private to the daemon.
3. The Runtime client depends only on public Runtime packages.
4. The `lilo` binary package uses binary distribution unless the project has a concrete crates.io use case.

Prefer placing the public Runtime wire contract in the existing `lilo-rm-core` package. Add another public package only if `lilo-rm-core` cannot own the contract without mixing protocol framing into the domain model.

Acceptance criteria:

1. `cargo metadata` reports no publishable package that depends on a private package.
2. `cargo package` succeeds for every package intended for crates.io.
3. `lilo-rm-client` has no Session dependency, direct or transitive.
4. The composed daemon keeps one private request envelope.

## Proposal 3: add the opaque launch payload

The governing architecture requires Session to attach an opaque capture lease or launch payload. Future Schedule forwards the value unchanged. Runtime carries the value to process launch without interpreting Transport semantics.

Add one domain neutral payload to the Session execution request. The type needs these properties:

1. Opaque contents at the Schedule and Runtime boundaries.
2. A version or kind that permits safe decoding by the owner.
3. No provider specific fields in Session, Schedule, or Runtime.
4. An absent value for launches that do not use Transport.
5. Stable serialization across the local socket and persisted intent when recovery requires it.

The final type name and encoding remain discussion items. The design should establish the data structure before provider handling code arrives.

Acceptance criteria:

1. The Session launch request carries the payload.
2. The current Runtime adapter forwards the payload unchanged.
3. A round trip test proves byte or value equality across the handoff.
4. A launch without a payload behaves exactly as it does at the reviewed commit.

## Proposal 4: restore the Session to Runtime boundary

`DaemonState` currently holds `RuntimePort`, `RuntimeService`, and `LifecycleStore`. The Session spawn handler writes Runtime lifecycle rows and appends Runtime events.

Keep the shared transaction and move Runtime operations behind a context owned port. The Session handler should depend on one execution interface. The current adapter may call Runtime directly. A future adapter may call Schedule without changing the Session handler.

Post commit Runtime event publication should be part of that interface. The Runtime implementation remains the event owner. Start with a direct port operation. Add an outbox only when a demonstrated retry or crash case requires one.

Acceptance criteria:

1. Session daemon does not depend on `lilo-runtime-store`.
2. Session handler state does not contain a concrete `RuntimeService`.
3. Runtime owns lifecycle mutations and Runtime event creation.
4. The shared transaction still makes intent and lifecycle evidence atomic.
5. No Running event becomes visible before the Session row commits.
6. The orphan termination and reconciliation tests still pass.

## Proposal 5: keep internal calls typed

`RuntimePort` converts `SessionId` to `&str`. The in-process adapter parses the value back into `SessionId`. `SpawnLaunch.target` and `ChildExit.session_id` repeat the same loss of type information.

Keep `SessionId`, `SpawnTarget`, and runtime signal types through internal calls. Parse text at the CLI, socket, configuration, and provider boundaries.

Acceptance criteria:

1. No in-process Runtime port method accepts a session identifier as `str` or `String`.
2. No internal launch request represents a validated target as `String`.
3. Socket adapters retain malformed input tests.
4. The typed port and wire adapter pass the same conformance cases.

## Proposal 6: correct active documentation

`NOTES/v1-v2-strategy.md` still describes SQLite and `SqlitePool`. Several architecture sections still describe completed migration phases as future work.

Update active documents to match Postgres and the composed daemon. Archive historical plans when their sequence remains useful. Keep the bounded context decisions and the reserved Schedule scope unchanged.

Acceptance criteria:

1. Active documents name Postgres as the database.
2. Active diagrams show one `lilod` process.
3. Historical phase labels appear only in archived material or explicit history sections.
4. `scripts/check-seam.sh` still passes.

## Proposed delivery sequence

Each change should end in a verifiable repository state.

1. **Topology subtraction.** Remove alternate production entrypoints, old distribution metadata, and stale topology prose.
2. **Publication repair.** Separate the public Runtime contract from the private composed envelope. Prove every published package with `cargo package`.
3. **Typed launch command.** Keep domain types across the in-process port and add the opaque payload.
4. **Ownership correction.** Move Runtime store and event work behind the execution port while preserving the two transaction protocol.
5. **Transport proof.** Implement the first provider traffic capture path through the established payload handoff.
6. **Canvas proof.** Read and command Session and Transport through `lilod` without direct storage access.

Schedule stays reserved throughout this sequence.

## Discussion points

The first discussion should resolve these choices:

1. Should the `lilo` binary package be published to crates.io, or only distributed as a binary?
2. Can `lilo-rm-core` own the public Runtime wire contract without mixing transport framing into domain types?
3. What exact opaque payload representation gives Transport versioning without provider fields outside Transport?
4. Should the current `RuntimePort` evolve into the future execution port, or should a new port replace it in the same change?
5. Which component retries post commit Runtime event publication after a crash?

My recommendations are binary only distribution for `lilo`, the existing Runtime core package for public Runtime contracts, and a direct execution port before any outbox. Those choices minimize new structure while keeping the future Schedule insertion local.

## Supporting evidence

The source reports contain detailed traces and candidate findings. This page is the synthesis. A candidate in a source report does not become an accepted change until this page or a later decision record accepts it.

| Document | Purpose |
| --- | --- |
| [Component flow](component-flow.md) | Implemented package graph, entrypoints, command routing, and complete spawn flow |
| [Data boundaries](data-boundaries.md) | Domain models, ownership, persistence, state machines, protocols, and validation |
| [Data boundary findings](data-boundaries-findings.md) | Detailed findings, explicit gaps, test evidence, and file counts |
| [Documentation and code drift](doc-code-drift.md) | Target comparison, publishability, cycles, line limits, and evolution readiness |
| [Comment review](comment-review.md) | Comments that hide stale phases, workarounds, unenforced rules, or ownership leaks |
| [Architecture audit](architecture-audit.sh) | Repeatable checks for package, topology, documentation, type, and ownership findings |

## Verification at the reviewed commit

The review used these checks:

```sh
fmm validate
docs/architecture/review/architecture-audit.sh
just check && just build && just test
```

`fmm validate` confirmed all 388 indexed files. The architecture audit passed. The repository gates passed. Build and test reported no relevant changes against `main` because the review itself made no source changes.

Update this page when the team accepts, changes, or rejects a proposal. Preserve the source reports as evidence for the reviewed commit.
