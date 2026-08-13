# lilo architecture vs repository: evolution readiness

Verified at HEAD `753e2ee91e3d41ae74dbe1cae3ec3ee6797e61da`
(`753e2ee docs(architecture): define Canvas, Schedule, and Transport boundaries`).
Read-only. Live files only. fmm used for topology, LOC, exports, and cycles;
authored sources cited below.

## 1. Target vs current

| Concern | Target (docs) | Current (HEAD) | Verdict |
| --- | --- | --- | --- |
| Product | One operator, one host, one `lilod` | True. `lilo daemon start` runs `compose::run_from_env` | Match |
| Contexts | Identity, Session, Schedule, Runtime, Transport; Canvas+Desktop as one product | Identity, Session, Runtime implemented. Schedule, Transport, Canvas, Desktop absent as code | Match on reservation; Session/Runtime docs stale on process topology |
| Launch path | Session -> Schedule -> Runtime; opaque payload + Transport lease | Session handler builds runtime spawn and calls `RuntimePort::spawn` in-process | Expected interim; no lease field exists |
| Join key | Typed UUIDv4 `SessionId` | `define_id!(SessionId)` generates v4; used as runtime spawn id | Match |
| Command surface | kubectl-shaped user verbs + operator namespaces; no identity CLI; no transport CLI yet | `lilo` implements the locked verb set; help hides identity | Match |
| Composition | After Phase 7, one `lilod` socket | Implemented in `internal/session/app/src/compose.rs` | Code ahead of session.md / runtime.md |
| Storage | Shared Postgres `LiloDb` | One migration `internal/db/migrations/0001_unified_schema.sql` | Match; runtime.md still describes Phase 3 imported pool |
| Identity | Authz + audit + RBAC shape; library layer only | Stub authorizer + audit rows; local-uid allow/deny | Partial; no RBAC |
| Transport | Capture lease, IR, overlay, fidelity | No crate, schema, command, or lease field | Reserved, as designed |
| Schedule | Sole placement; restart policy; stable pane ids | No crate, schema, command, restart type | Reserved, as designed |
| Canvas | Consumes `lilod` read/command models | `apps/` is a one-line placeholder | Reserved, as designed |

Target dependency from `docs/architecture/system.md:54-61`:

```text
Canvas or lilo -> Session -> Schedule -> Runtime
                      |
                      +-> Transport -> provider
```

Implemented dependency (composed daemon):

```text
lilo -> LilodRpc socket
          |- SessionService --InProcessRuntime--> RuntimeService
          `- RuntimeService (diagnostic RuntimeRpc)
IdentityClient (stub) authorizes both
```

No Schedule node. No Transport node. No capture lease on either `SpawnRequest`.

## 2. Reserved vs implemented

### Implemented

| Context | On-disk | LOC (fmm, all files) | Notes |
| --- | --- | --- | --- |
| Session | `internal/session/{app,core,daemon,driver,store}` | 177 files / 26,480 | Five-subdir shape matches `CLAUDE.md:97-98` and `session.md:201-207` |
| Runtime | `internal/runtime/{app,daemon,launchers,store}` + published `lilo-rm-*` | 113 files / 17,124 plus crates | Matches `runtime.md:167-176` |
| Identity | published `lilo-im-{core,store,stub}` + `internal/identity/service` | 186 LOC internal + 606/620/168 crate LOC | Thin adapter over stub |
| Shared | `internal/{db,wire,port}`, `crates/{lilo,lilo-common,lilo-paths,lilo-sys}` | present | Composition, ids, paths, PAL |

Workspace members in `Cargo.toml:2-28` match this set. No `schedule`, `transport`, or `canvas` member.

### Reserved (empty or docs only)

| Target | Evidence |
| --- | --- |
| Schedule | `docs/architecture/schedule.md:3-4` says no crate/daemon/schema/command. Confirmed: no `internal/schedule`, no `RestartPolicy` symbol, no `lilo schedule` verb. `CLAUDE.md:51-55` forbids expanding it during the first Transport/Canvas proof. |
| Transport | `docs/architecture/transport.md:3-4`. No implementation files. fmm `term=transport` hits only session MCP `transport.rs` (stdio MCP), not provider wire. `NOTES/transport-integration.md` is marked superseded. |
| Canvas / Desktop | `docs/architecture/canvas.md:3-4`. `apps/README.md` is one reserved line. Same for `packages/`, `python/`, `helix/`, `products/`, `infrastructure/`. |
| Identity CLI | `crates/lilo/src/cli.rs:336-343` asserts help does not contain `identity`. |
| `lilo transport` | Command enum in `crates/lilo/src/cli.rs:234-327` has no transport variant. |

Justified. The first vertical slice is supposed to prove Transport+Canvas on the existing Session->Runtime route without activating Schedule (`system.md:124-126`, `schedule.md:144-145`).

## 3. Layering consistency

### What holds

- User verbs live on `lilo` and dispatch into session app (`crates/lilo/src/cli.rs:84-117`).
- Operator `lilo runtime …` is a separate namespace (`cli.rs:118-123`, help test `427-448`).
- `lilo capture` is pane capture, not provider capture (`NOTES/transport-integration.md:43`, session app `cli/capture.rs`).
- `SessionId` is the runtime spawn id (`crates/lilo-rm-core/src/types/spawn.rs:206-207`, `CLAUDE.md:184-185`).
- Selector prefix exists (`internal/session/core/src/selector/types.rs:17-19`, floor 4 hex; human short floor is 7 in `lilo-common/src/id.rs:4`).
- Identity is a library, used by both session spawn (`handler/spawn.rs:107-114`) and runtime RPC (`internal/runtime/daemon/src/identity.rs:10-33`).
- Session driver converts session launch to runtime request without interpreting provider payloads (`internal/session/driver/src/conv.rs:22-38`). Fields are runtime execution fields only.

### Drift: process topology

`session.md:124-126` and `runtime.md:113-115` still say Phase 3/4 keep separate daemons and Phase 6/7 will fold verbs and compose `lilod` later.

Code already did both:

- Unified verb tree: `crates/lilo/src/cli.rs:234-327`.
- Composed daemon: `internal/session/app/src/compose.rs:125-133` builds `RuntimeService` then `SessionService` on one `LiloDb`, binds one socket, and demuxes `LilodRpc` (`compose.rs:237-251`).
- Operator entry: `crates/lilo/src/cli/daemon.rs:52`.

README (`README.md:13-14`, `16-32`) matches the code. session.md and runtime.md describe an earlier phase.

### Drift: two session servers

`SessionService` + `compose.rs` is the live `lilod` path.

`internal/session/daemon/src/server.rs` is a second daemon:

- still builds `RuntimeService` in-process (`server.rs:41-48`)
- binds the same path style of socket (`server.rs:35-36`)
- decodes raw `SessionRpc`, not `LilodRpc` (`server.rs:132`)
- cannot serve `lilo runtime` on that socket
- still exported (`internal/session/daemon/src/lib.rs:26`)
- production `lilo` / `sm` do not call it; `tests/server_concurrency.rs` does

This is a leftover Phase 4 accept loop beside the Phase 7 composer.

### Drift: Session writes Runtime store

`DaemonState` holds `lilo_runtime_store::LifecycleStore` (`internal/session/daemon/src/handler/state.rs:19-22`, `52`). Spawn commits lifecycle rows in the session transaction (`handler/spawn.rs:12`, `102-103`).

Runtime still owns the table (`0001_unified_schema.sql:130-145`). Session reaching into that store is a composition shortcut. It works because both share `LiloDb`. It will fight Schedule later, when Session should persist placement evidence returned through Schedule, not Runtime rows.

### Drift: published runtime client bound to composed envelope

`lilo-rm-client` is a published crate (`crates/lilo-rm-client/Cargo.toml` has no `publish = false`; default true). It writes `LilodRpc::Runtime(rpc)` (`crates/lilo-rm-client/src/lib.rs:18`, `306`).

`lilo-wire` is unpublished (`internal/wire/Cargo.toml:10`) and depends on `lilo-session-core` (`internal/wire/Cargo.toml:17-18`). The published runtime client therefore depends on the unpublished session protocol enum.

That is composition leaking into the public runtime crate. A standalone `rtmd` socket would no longer match the client.

### Historic names still on published surfaces

- `lilo-rm-core` description: "Runtime Matters … rtmd clients" (`crates/lilo-rm-core/Cargo.toml:3`)
- `lilo-rm-client` crate docs: "public rtmd JSON line contract" (`src/lib.rs:3-7`)
- `lilo-im-core` crate docs: "Identity Matters" and a v2 `lilo-im-daemon` (`src/lib.rs:1-3`)
- Diagnostic binaries still named `sm` and `rtm` (`internal/session/app/Cargo.toml:19-21`, `internal/runtime/app/Cargo.toml:20-22`)
- Both internal apps set `[package.metadata.dist] dist = true` while `publish = false`

`CLAUDE.md:39-41` says the public brand is littleorgans / `lilo`. Historic names on crates.io descriptions and cargo-dist binary names are leftover import seams.

## 4. Publishability boundaries

`CLAUDE.md:98`: "`crates/` contains published crates only. `internal/` contains non-published substrate."

| Package | Path | `publish` | Problem |
| --- | --- | --- | --- |
| `lilo` | `crates/lilo` | `true` (`Cargo.toml:9`) | Runtime deps include unpublished `lilo-runtime-app`, `lilo-session-daemon`, `lilo-session-core`, `lilo-session-app`, `lilo-db` (`crates/lilo/Cargo.toml:16-25`) |
| `lilo-rm-client` | `crates/` | default `true` | Depends on unpublished `lilo-wire` |
| `lilo-rm-core` | `crates/` | default `true` | Clean of internal deps. Publishable. |
| `lilo-im-core` | `crates/` | default `true` | Publishable. |
| `lilo-im-store` | `crates/` | default `true` | Publishable; `postgres` feature optional. |
| `lilo-im-stub` | `crates/` | default `true` | Publishable. |
| `lilo-common` | `crates/` | `true` | Depends on published `lilo-paths` only. |
| `lilo-paths` | `crates/` | `true` | Publishable. |
| `lilo-sys` | `crates/` | `true` | Publishable. |
| `lilo-build-support` | `crates/` | `false` (`Cargo.toml:10`) | Lives under `crates/` but is unpublished tooling |
| All `internal/*` | internal | `false` | Matches policy |
| `xtask`, `lilo-integration-tests` | tools/tests | `false` | Correct |

crates.io cannot publish `lilo` or `lilo-rm-client` at this graph. That is acceptable for a private pre-release if Phase 8 `mirror-publish` rewrites the graph. It is not acceptable if `publish = true` is read as "ready to cargo publish."

`xtask dist-check` and `xtask mirror-publish` are explicit deferrals (`tools/xtask/src/main.rs:54-58`).

## 5. Dependency cycles

fmm `fmm_dependency_cycles(filter=source, explain=true)` reports no workspace-crate cycles. Three intra-crate SCCs:

1. `crates/lilo-rm-core/src/lib.rs` <-> `types/lifecycle.rs`
   Facade re-export cycle. Harmless if `include_mod_hierarchy` is the cause; still a rustc module cycle.

2. `internal/runtime/daemon/src/{api.rs, handler.rs, service.rs}`
   `api -> service -> handler -> api`. Composition hook and RPC dispatch share one SCC. Real coupling: `RuntimeService` cannot be understood without the handler, and the handler cannot be understood without the service.

3. `internal/session/core/src/tool_contracts/{contract,metadata,params,raw,render}.rs`
   Dense module cycle inside generated-contract support. Local only.

Session -> Runtime is one-way at crate level (`lilo-session-daemon` depends on `lilo-runtime-daemon` and `lilo-runtime-store`; runtime crates do not depend on session). That is the interim launch path, not a cycle.

## 6. Line limit risks

`scripts/check-loc-limit.sh` enforces 700 lines on `*.rs` (and future TS/JS/Py). No file exceeds 700. Closest source files (wc -l):

| Lines | File |
| --- | --- |
| 673 | `internal/runtime/daemon/src/tmux_nudge.rs` |
| 643 | `internal/runtime/daemon/src/api.rs` |
| 599 | `crates/lilo/src/cli.rs` |
| 582 | `internal/session/store/src/postgres/mail.rs` |
| 565 | `internal/session/app/src/cli/output.rs` |
| 553 | `internal/session/daemon/src/handler/messaging.rs` |
| 548 | `internal/runtime/daemon/src/event_log.rs` |
| 536 | `crates/lilo-rm-core/src/proto.rs` |
| 514 | `crates/lilo/src/cli/generated_schema.rs` (generated) |

Closest tests: `mail_safety.rs` 648, `handler_messaging.rs` 621, `session_spawn_contract.rs` 612, `port_conformance.rs` 604.

Function-size risk: `impl DaemonState` in `handler/spawn.rs` is lines 23-337 (315 lines in one impl block). `spawn` itself is 24-94. The hard ~150 line function rule is not yet breached on `spawn`, but the impl is already a dump.

First Transport slice will want new fields on spawn, launch conversion, and daemon state. Touch `tmux_nudge.rs` or `api.rs` only after splitting.

## 7. Strengths

1. Bounded-context docs exist, are short, and agree with `CLAUDE.md` / `README.md` on ownership (Session intent, Runtime execution, Schedule placement, Transport wire, Canvas presentation).
2. Repository layout matches the locked five-subdir Session shape and Runtime crate map.
3. Typed id family is real and used (`crates/lilo-common/src/id.rs:22-96`, `define_id!` for SessionId/MessageId/IntentId/AuditId; `new()` is v4 at lines 43-45; test at 132-136).
4. Phase 6 command surface and Phase 7 `lilod` composition are implemented, not sketched.
5. One Postgres schema with an explicit owner seam (`0001_unified_schema.sql:3-7`, `9-25`, `30-50`, `115-145`).
6. `RuntimePort` plus `InProcessRuntime` / `RtmdDriver` and `port_conformance.rs` give a tested seam for swapping the Session->Runtime hop later.
7. `lilo-port` opaque error model (`internal/port/src/lib.rs`) matches `NOTES/bounded-context-port-error-model.md`.
8. Gates exist and are wired: `moon.yml` runs fmt, clippy, loc, provenance, seam, env; `.github/workflows/pr.yml:84-94` runs `moon ci` plus ignored DB tests.
9. Generated CLI/MCP surfaces have one xtask codegen path (`tools/xtask/src/main.rs:26-76`) and guard tests.
10. Schedule/Transport/Canvas are actually absent, not half-imported. That is the correct reservation.

## 8. Structural risks

1. **Docs describe a past topology.** session.md / runtime.md still teach separate daemons and future Phase 6/7. New work following those docs will recreate sockets that `compose.rs` already replaced.

2. **Capture lease is documented as current and is not implemented.**
   `CLAUDE.md:64-66`: "Session prepares capture for the typed UUIDv4 `SessionId` and attaches an opaque capture lease to the launch payload. The current v0.8 path passes that payload directly to Runtime."
   Session `SpawnRequest` (`internal/session/core/src/proto/spawn.rs:11-38`) has runtime, role, workspace, dir, namespace, target, agent_config, isolation, image, env, mounts, shell_resume, labels, force.
   Runtime `SpawnRequest` (`crates/lilo-rm-core/src/types/spawn.rs:206-223`) has session_id, runtime, isolation, image, env, mounts, cwd, target, force, shell_resume.
   Conversion copies those fields only (`conv.rs:22-38`). No lease, no opaque blob, no Transport hook.

3. **Two accept loops** (`compose.rs` vs `session/daemon/src/server.rs`) plus two runtime drivers. Production uses in-process. Tests still exercise `RtmdDriver` against a socket. Easy to "fix" the wrong one.

4. **Session owns a Runtime store handle.** Blocks a clean Schedule cutover and invites Session to reconcile Runtime rows.

5. **Publish graph is a lie** for `lilo` and `lilo-rm-client`. `LilodRpc` in the published client freezes composition into the public runtime crate.

6. **Identity is a stub with a local-uid check**, while README says "RBAC" (`README.md:24-26`). `lilo-im-core/src/lib.rs:2` says authorization is not enforced in v1. `IdentityClient::authorize_in_tx` (`internal/identity/service/src/client.rs:52-73`) allows only `AuditDecision::evaluate_local`. Fine for one operator; do not treat it as RBAC.

7. **Positional tmux addresses are stored.** `session_sessions.tmux_pane` and `runtime_lifecycle.tmux_pane` are TEXT (`0001_unified_schema.sql:43`, `139`). `TmuxAddress` is `session:window.pane` (`crates/lilo-rm-core/src/types/spawn.rs:12-22`). Schedule forbids reconstructing identity from position (`schedule.md:62-64`). Stored display addresses will become a migration if treated as bindings.

8. **Historic `sm` / `rtm` binaries** remain cargo-dist candidates. Brand and operator surface diverge.

9. **Near-cap files** sit on the spawn/nudge/API path the first slice must extend.

## 9. Misplaced ownership

| Item | Lives in | Should live in | Evidence |
| --- | --- | --- | --- |
| Composed daemon accept loop | session app `compose.rs` | Correct per `CLAUDE.md:86-88` (composition rooted in Session app) | Keep |
| Legacy session accept loop | `session/daemon/src/server.rs` | Delete or test-only after compose is the sole path | `lib.rs:26` still exports it |
| Runtime lifecycle writes during session spawn | session daemon | Runtime service, or later Schedule consuming Runtime evidence | `handler/state.rs:19-22` |
| `LilodRpc` envelope | unpublished `lilo-wire`, imported by published `lilo-rm-client` | internal client, or a published envelope that does not depend on session-core | `lilo-rm-client/src/lib.rs:306` |
| Diagnostic `sm`/`rtm` names | internal app bins | `lilo` only on the public surface; keep hidden shims | Cargo.toml `[[bin]]` names |
| MCP stdio named `transport` | `internal/session/app/src/mcp/transport.rs` | Fine as MCP transport; name collides with the Transport context | fmm search |

## 10. Needless indirection

Worth keeping:

- `RuntimePort` + conversion + conformance tests. This is the Schedule cutover seam.
- `IdentityPort` around `IdentityClient`. Thin, one impl.
- `lilo-port` opaque errors. Small and used.
- `LilodRpc` as a one-enum envelope (`internal/wire/src/lib.rs:5-8`). Eight lines. Correct for one socket.

Worth deleting or shrinking:

- `session/daemon/src/server.rs` once tests move to compose (or to `SessionService` directly). It rebuilds the same `RuntimeService`+`InProcessRuntime` stack and speaks a different wire.
- `SessionServiceContext::from_env` (`service.rs:40-49`) also builds its own `RuntimeService`. compose.rs does the same. Two composition entrypoints.
- `RtmdDriver` as a production type. Keep for tests until the socket client is only diagnostic `lilo runtime`.

Not needless:

- Two `SpawnRequest` types (session vs runtime). Different bounded contexts. Do not merge them. Add an opaque payload / lease on the session side and forward bytes on the runtime side.

## 11. Justified reservations

- **Schedule** remains off. First proof is allowed to use the direct Session->Runtime path (`system.md:124-126`). Do not add topology crates, pane identity types, or restart policy now.
- **Transport implementation language, process topology, storage owner, failure policy** are explicitly open (`transport.md:43-46`, `158-160`; `CLAUDE.md:67-69`).
- **Canvas host and HTML-vs-direct delivery** are Stuart-owned (`system.md:138-147`, items 2, 5, 6, 8).
- **Identity verbs** (`whoami` / `can-i` / audit CLI) wait until Identity owns real verbs (`CLAUDE.md:118-121`).
- **apps / packages / python** wait for the Transport+Canvas proof (`README.md:92`).
- **xtask dist-check / mirror-publish** wait for Phase 8 (`tools/xtask/src/main.rs:54-58`).
- **No EventId / NamespaceId / ScheduleId** until a stored field needs one (`CLAUDE.md:182-183`, `schedule.md:66-68`). Code follows this.

## 12. Likely next vertical-slice seams

Smallest coherent proof (`system.md:112-126`):

1. `lilo run claude` session-backed, capture required
2. Transport holds first Claude Messages request
3. Canvas first-turn report
4. One tool-description edit by tool name
5. Validate, forward, show original / forwarded / response / audit

Code seams that already exist and should be extended, not replaced:

| Seam | Where | What to add |
| --- | --- | --- |
| Session mint + intent | `handler/spawn.rs:29-70` | After `SessionId::new()`, call a Transport port; persist lease on the intent |
| Opaque launch payload | session `SpawnRequest` / `SpawnLaunch` / `runtime_spawn_request` | One opaque field Session does not interpret. Runtime applies launch spec only. |
| Runtime execution | `InProcessRuntime::spawn` (`in_process.rs:49-64`) -> `RuntimeService::spawn` | Pass through env/socket/path additions from the lease without parsing them |
| Composed socket | `LilodRpc` (`wire/src/lib.rs:5-8`) | Add `Transport(...)` only when Transport has real RPCs. Do not add Schedule. |
| Canvas reads | none | New client of Session+Transport read models through `lilod`. Do not read SQL. |
| Fixtures | Transport Matters at `1d5c9b72` (reference only) | Copy behavior, not packages (`transport.md:125-146`) |

Do not:

- import Transport Matters launcher topology
- add `internal/schedule`
- store positional overlay ids
- make `lilo capture` mean provider capture
- teach Session to parse Messages JSON

Open product forks that block implementation, not structure: Claude-only vs both providers editable; blocking vs passive first request; capture failure posture; HTML-first vs Canvas-first (`system.md:138-147`).

## 13. Explicit unknowns

1. Is `session/daemon/src/server.rs` kept as a test double or is it dead production code? No production caller found outside tests.
2. Will `lilo` ever be published to crates.io as-is, or only as a binary via cargo-dist from the private repo? `publish = true` plus unpublished deps cannot both be true.
3. Should `lilo-rm-client` speak raw `RuntimeRpc` (standalone runtime) or `LilodRpc` (composed daemon)? Today it speaks the latter and cannot talk to `runtime/daemon` `run_daemon` without the envelope.
4. Who owns Transport tables? `transport.md:72-74` leaves Postgres table ownership unlocked.
5. Whether `sm` / `rtm` remain cargo-dist artifacts (`metadata.dist = true`).
6. Whether `tmux_pane` TEXT is already treated as identity anywhere outside display/status. Not fully audited beyond schema and `TmuxAddress`.
7. fmm index `.fmm.db` mtime is older than HEAD. Structural counts came from fmm; every ownership claim was re-checked on live files.

## 14. Evolution readiness

The repo can take the first Transport+Canvas slice on the current Session->Runtime route.

It is not ready to activate Schedule. It is not ready to cargo-publish the `lilo` crate family. It is not ready to treat Identity as RBAC.

Highest-leverage prep before the slice, in order:

1. Treat `compose.rs` + `SessionService` as the only daemon story. Update session.md / runtime.md Phase 3/4/7 language to match README. Quarantine or delete `server.rs` as a second accept loop.
2. Add an opaque capture-lease / launch-addition field on the session launch path now, empty and ignored, so Transport can fill it without a later Runtime protocol break. This is the sentence `CLAUDE.md:64-66` already claims.
3. Stop putting `LilodRpc` in the published `lilo-rm-client`, or mark that crate unpublished until the envelope is a published contract that does not depend on `lilo-session-core`.
4. Split `tmux_nudge.rs` / `api.rs` before the slice lands more code there.
5. Leave Schedule, Canvas package layout, and Transport Matters packages untouched.

## Method

- HEAD: `git rev-parse HEAD`
- Docs: `docs/architecture/{system,session,runtime,schedule,transport,canvas}.md`, `CLAUDE.md`, `README.md`, `NOTES/transport-integration.md`
- Topology: fmm `list_files` group-by subdir; crate `Cargo.toml` publish flags; `Cargo.toml` workspace members
- Cycles: fmm `dependency_cycles` source + explain
- LOC: fmm source listing + `wc -l` (includes tests)
- Symbols: fmm lookup `SessionService`, `RuntimeService`, `SpawnRequest`, `SessionId`; live reads of spawn, compose, wire, client, identity, schema
- Absence checks: find for transport/schedule/canvas paths; fmm search for `RestartPolicy`, `capture lease`, `transport`
