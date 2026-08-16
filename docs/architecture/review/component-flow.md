# littleorgans Implemented Component Graph and Control Flow

## Review scope

- Repository: littleorgans monorepo root
- Pinned HEAD: `753e2ee91e3d41ae74dbe1cae3ec3ee6797e61da`
- Branch: `main`, aligned with `origin/main`
- Mode: read only architecture review
- Structural evidence: fmm indexed 271 source files and 37,374 lines of code. The repository carries `.fmmrc.toml` and local `.fmm.db` navigation state.
- Workspace evidence: `cargo metadata --no-deps` reports 25 workspace packages, including 9 publishable packages and 16 internal or test packages.

## Executive summary

The implemented product is a single Rust control plane. The `lilo` CLI sends typed Session or Runtime JSON line requests to one `lilod` Unix socket. The daemon composes one shared Postgres pool, one `RuntimeService`, one `SessionService`, and local UID based Identity authorization in one process.

The normal user launch path is Session backed and in process after the socket boundary: `lilo -> Session RPC -> Session intent transaction -> InProcessRuntime -> RuntimeService -> shim -> Claude or Codex`. Raw `lilo runtime ...` commands use the Runtime socket protocol and remain outside Session records. Schedule, Transport, Canvas, and Desktop are documented target contexts with no current workspace package or implementation.

The strongest implemented pattern is the two transaction spawn intent. Session commits authorization, the pending intent, and Runtime's `Forking` row before the process side effect. It commits the Session row, `Running` lifecycle, and resolved intent after Runtime reports ready. Several current couplings matter for the target migration: Session keeps both an abstract `RuntimePort` and a concrete `RuntimeService`; `logs` reads a Runtime produced file directly; the port stringifies typed IDs; and the architecture diagrams still describe the inactive socket based `RtmdDriver` as the current Session path.

## Project metadata

| Item | Current fact | Evidence |
| --- | --- | --- |
| Product | Local first coding agent control plane, one operator, one host, one daemon | `README.md:3-14` |
| Version | Workspace version `0.8.0`, pre release | `Cargo.toml:31-38`, `README.md:6-7` |
| Language | Rust 2024 edition | `Cargo.toml:31-38` |
| Toolchain | Rust 1.95 with Clippy and rustfmt | `rust-toolchain.toml:1-3` |
| Async runtime | Tokio 1.48 | `Cargo.toml:79-91` |
| CLI | Clap 4.5 | `Cargo.toml:41-45` |
| Persistence | Postgres through sqlx 0.8.3 | `Cargo.toml:76-76`, `internal/db/src/lib.rs:42-61` |
| Serialization | Serde and JSON line RPC | `Cargo.toml:74-75`, `internal/wire/src/lib.rs:1-8` |
| Build | Cargo is authoritative; Just is the operator surface; Moon runs CI | `README.md:97-112`, `moon.yml:29-77` |
| Runtime targets | Claude and Codex, host or Docker isolation, tmux or headless target | `internal/runtime/launchers/src/lib.rs:57-69`, `internal/runtime/daemon/src/backend.rs:33-53`, `internal/runtime/daemon/src/shim_socket.rs:25-35` |

## Repository and package topology

The root workspace lists every active Rust member. No Schedule or Transport crate appears in the member list. Canvas application workspaces remain placeholders (`Cargo.toml:1-28`, `README.md:84-95`).

fmm source topology:

| Area | Files | Lines | Main role |
| --- | ---: | ---: | --- |
| `internal/session` | 121 | 15,654 | Session app, protocol, daemon, runtime bridge, Postgres store |
| `internal/runtime` | 74 | 10,453 | Runtime app and shim, daemon, launchers, Postgres lifecycle store |
| `internal/db` | 3 | 677 | Shared Postgres pool, migrations, transaction helpers |
| `internal/identity` | 2 | 93 | Composed Identity client |
| `internal/wire` | 1 | 8 | Top level `LilodRpc` envelope |
| `crates` | 68 | 9,948 | Published CLI, common, paths, identity model, Runtime protocol/client, OS primitives |

### Implemented component graph

```text
lilo binary
  |-- Session user verbs ------------------------------+
  |                                                    |
  |    lilo-session-app                                |
  |      -> SessionRpc                                 |
  |      -> LilodRpc::Session over lilod.sock          |
  |                                                    v
  |                                         lilo-session-daemon
  |                                           |-- IdentityPort
  |                                           |     -> IdentityClient
  |                                           |     -> StubAuthorizer
  |                                           |     -> identity_audit
  |                                           |-- SessionStore
  |                                           |     -> session_* and mail tables
  |                                           |-- RuntimePort
  |                                           |     -> InProcessRuntime
  |                                           |     -> RuntimeService
  |                                           +-- concrete RuntimeService
  |                                                 -> post commit event append
  |
  +-- lilo runtime operator verbs
         -> lilo-runtime-app
         -> lilo-rm-client
         -> LilodRpc::Runtime over the same lilod.sock
         -> Runtime wire handler and Identity authorization
         -> RuntimeService domain path

RuntimeService
  |-- Runtime launchers -> Claude or Codex LaunchSpec
  |-- Runtime backend -> host or Docker preparation
  |-- SpawnCoordinator -> pending LaunchSpec and ShimReady channel
  |-- shim bootstrap -> tmux respawn or headless child
  |-- LifecycleStore -> runtime_lifecycle in Postgres
  +-- EventLog -> JSONL runtime event stream

Shared foundations
  |-- LiloDb -> one Postgres pool and one migration
  |-- lilo-common -> typed UUIDv4 IDs, diagnostics, logging
  |-- lilo-paths -> ~/.lilo path and environment policy
  |-- lilo-wire -> Session or Runtime top level socket envelope
  +-- lilo-sys -> Unix socket, peer credentials, process and signal primitives
```

### Package dependency direction

1. `crates/lilo` is the public binary. It depends on Session app, Session daemon, Session core, Runtime app, Runtime core, paths, common, and the database package (`crates/lilo/Cargo.toml:15-30`).
2. `lilo-session-app` owns CLI and daemon composition. Its production dependencies include `lilo-session-daemon`, `lilo-runtime-daemon`, `lilo-wire`, and `lilo-db` (`internal/session/app/Cargo.toml:23-42`).
3. `lilo-session-daemon` depends on Session core, driver, store, Runtime daemon, Runtime store, Runtime client, Identity service, and the shared database (`internal/session/daemon/Cargo.toml:16-41`). This is the main composition layer rather than a domain only layer.
4. `lilo-session-driver` defines `RuntimePort` and provides two adapters. `InProcessRuntime` calls `RuntimeService`; `RtmdDriver` calls `RuntimeClient` (`internal/session/driver/Cargo.toml:16-25`, `internal/session/driver/src/lib.rs:3-21`).
5. `lilo-runtime-daemon` depends inward on Runtime core, Runtime launchers, Runtime store, Identity service, paths, OS primitives, and the shared database (`internal/runtime/daemon/Cargo.toml:19-37`).
6. `lilo-session-store` and `lilo-runtime-store` depend on domain contract crates plus `lilo-db`. Neither store depends on its daemon (`internal/session/store/Cargo.toml:16-26`, `internal/runtime/store/Cargo.toml:16-26`).
7. `lilo-identity-service` depends on Identity core, the Postgres audit store, and the local stub authorizer (`internal/identity/service/Cargo.toml:17-25`).
8. `lilo-wire` contains only the top level tagged envelope. Domain request and response contracts remain in Session core and Runtime core (`internal/wire/src/lib.rs:1-8`).

The direction mostly follows `app -> daemon -> driver/store -> core/foundation`. The main exception is deliberate composition coupling: Session daemon imports Runtime daemon and Runtime store directly.

## Entry points and daemon composition

`crates/lilo/src/main.rs` parses `Cli`, initializes logging, runs the selected command, renders a typed diagnostic, and maps it to an exit code (`crates/lilo/src/main.rs:13-29`). The root CLI routes every user verb into `lilo-session-app`; the raw `runtime` namespace goes to `lilo-runtime-app`; `daemon start` goes to the composed daemon (`crates/lilo/src/cli.rs:77-135`).

`lilo daemon start` invokes `lilo_session_app::compose::run_from_env` (`crates/lilo/src/cli/daemon.rs:42-55`). The composed service:

1. Opens one resolved Postgres pool and runs the unified migration.
2. Builds `RuntimeService` with that pool.
3. Builds `SessionService` with the same pool and an `Arc<RuntimeService>`.
4. Reconciles pending Session spawn intents.
5. Binds one Unix socket and accepts connections.
6. Routes `LilodRpc::Runtime` to `RuntimeService::handle_rpc` and `LilodRpc::Session` to `SessionService::handle_rpc` after peer credential extraction.

Evidence: `internal/session/app/src/compose.rs:115-170`, `internal/session/app/src/compose.rs:206-254`.

`RuntimeService::build` prepares Runtime state and starts reconciliation (`internal/runtime/daemon/src/service.rs:56-67`). Bootstrap warms the launcher registry and constructs `LifecycleStore` plus `IdentityClient` from the shared pool (`internal/runtime/daemon/src/server/bootstrap.rs:36-47`).

`SessionService::build` constructs `SessionStore`, the current `InProcessRuntime` adapter, a Session Identity port, lifecycle processing, and Runtime event processing (`internal/session/daemon/src/service.rs:69-94`).

## User verb routing

The root Session command switch is `internal/session/app/src/cli.rs:41-58`. The daemon's typed request switch is `internal/session/daemon/src/handler/dispatch.rs:56-123`.

| User surface | Implemented path | State and authority |
| --- | --- | --- |
| `lilo run`, `lilo create session` | Session app builds `SessionRpc::Spawn`; Session daemon performs the two transaction launch described below | Session mints `SessionId`, authorizes Spawn, owns intent and Session row; Runtime owns lifecycle and process |
| `lilo get session` and namespace reads | Session `List` or namespace RPC, then `SessionStore` selector query | List and read authorize at the Session door (`internal/session/daemon/src/handler/authz.rs:21-29`); rows come from Session store (`internal/session/daemon/src/handler/sessions.rs:18-29`) |
| `lilo delete session` | Resolve selector, authorize each session, call `RuntimePort::terminate`, persist terminal evidence | `internal/session/daemon/src/handler/sessions.rs:60-130` |
| `lilo label` | Resolve target, authorize `Link`, mutate labels in Session store | `internal/session/daemon/src/handler/sessions.rs:76-89`, `internal/session/daemon/src/handler/sessions.rs:132-149` |
| `lilo mail` | Resolve recipients, authorize mailbox action, persist `messages` and `message_deliveries`; optional notification uses Runtime nudge | `internal/session/daemon/src/handler/messaging.rs:26-67`, `internal/session/daemon/src/handler/messaging.rs:185-214`, `internal/session/daemon/src/handler/messaging.rs:242-298` |
| `lilo nudge` | Resolve Session recipient, authorize `Nudge`, call `RuntimePort::nudge` | `internal/session/daemon/src/handler/messaging.rs:106-127`, `internal/session/daemon/src/handler/messaging.rs:161-183` |
| `lilo capture` | Load Session row, authorize Read, call `RuntimePort::capture`, then Runtime captures tmux scrollback | `internal/session/daemon/src/handler/sessions.rs:31-58`, `internal/session/driver/src/in_process.rs:83-100`, `internal/runtime/daemon/src/server/state.rs:200-223` |
| `lilo logs` | Resolve Session and authorize Logs, then read the stored transcript path from the local filesystem | `internal/session/daemon/src/polish.rs:15-43`, `internal/session/daemon/src/polish.rs:137-146` |
| `lilo wait` | Poll SessionStore until the stored state or count matches | `internal/session/daemon/src/polish.rs:92-108`, `internal/session/daemon/src/polish.rs:148-160` |
| `lilo doctor` | Authorize Doctor, reconcile current evidence, call Runtime doctor, inspect lost Session rows | `internal/session/daemon/src/polish.rs:45-90`, `internal/session/daemon/src/polish.rs:123-134` |
| `lilo mcp` | MCP JSON RPC reaches the Session daemon bridge; tools call the same `DaemonState::handle_direct` with Session RPC variants | `internal/session/daemon/src/mcp_bridge.rs:56-71`, `internal/session/daemon/src/mcp_tools/agent.rs:30-86` |
| `lilo runtime ...` | Runtime app uses `RuntimeClient`, writes `LilodRpc::Runtime`, authorizes in Runtime's wire handler, then calls the same Runtime domain helpers | `internal/runtime/app/src/cli.rs:107-117`, `crates/lilo-rm-client/src/lib.rs:253-266`, `crates/lilo-rm-client/src/lib.rs:301-316`, `internal/runtime/daemon/src/handler.rs:127-193` |

Mail and nudge intentionally differ. Mail is durable Session state. Nudge is ephemeral Runtime delivery (`docs/architecture/session.md:73-75`).

## Complete session backed launch flow

The following is the current `lilo run` path at HEAD.

### 1. CLI request construction

The root CLI maps `Command::Run` to Session app dispatch (`crates/lilo/src/cli.rs:84-117`, `internal/session/app/src/cli.rs:41-58`). The Session app:

- resolves namespace and canonical working directory;
- normalizes agent configuration;
- captures caller environment and optional shell resume state;
- parses labels, mounts, isolation, image, target, and force;
- sends `SessionRpc::Spawn` to the daemon.

Evidence: `internal/session/app/src/cli/run.rs:41-99`.

`lilo create session` uses the same helper with `target = headless`, default isolation, no image, no mounts, and no force (`internal/session/app/src/cli/run.rs:29-39`).

### 2. Socket framing and principal

The Session client connects to the configured daemon endpoint and writes `LilodRpc::Session(request)` as JSON plus newline (`internal/session/daemon/src/socket.rs:11-21`). The composed listener extracts Unix peer credentials, then routes the request to `SessionService` (`internal/session/app/src/compose.rs:206-250`).

### 3. Session request normalization

Session dispatch sends `SessionRpc::Spawn` to `DaemonState::spawn` (`internal/session/daemon/src/handler/dispatch.rs:56-72`). Spawn then:

1. Mints a typed UUIDv4 `SessionId`.
2. Normalizes namespace, directory, and request context.
3. Resolves optional agent configuration.
4. Builds `SpawnLaunch` and the Runtime `SpawnRequest`.
5. Prepares a draft Session and a durable pending intent.

Evidence: `internal/session/daemon/src/handler/spawn.rs:23-70`. The typed ID macro centralizes v4 generation and transparent wire representation (`crates/lilo-common/src/id.rs:22-96`).

Session strips caller supplied `LILO_AGENT_*` entries, then injects authoritative session ID, role, and workspace. The Runtime launcher also injects runtime kind (`internal/session/daemon/src/handler/spawn.rs:369-406`, `internal/runtime/launchers/src/lib.rs:97-108`).

### 4. Transaction A: intent before side effect

Session begins one Postgres transaction and performs these operations together:

1. Identity evaluates and records Spawn authorization against workspace, role, runtime, SessionId, and labels.
2. Session inserts a pending `session_spawn_intents` row.
3. Runtime lifecycle store inserts a `Forking` row for the same SessionId.
4. The transaction commits before process creation.

Evidence: `internal/session/daemon/src/handler/spawn.rs:96-136`. `IdentityClient::authorize_in_tx` writes the audit row on the same transaction and returns the local decision (`internal/identity/service/src/client.rs:52-74`).

### 5. In process Runtime bridge

Session invokes `RuntimePort::spawn` (`internal/session/daemon/src/handler/spawn.rs:70-79`). The current adapter reparses the string SessionId, converts `SpawnLaunch` to Runtime's `SpawnRequest`, and calls `RuntimeService::spawn` directly (`internal/session/driver/src/in_process.rs:49-65`, `internal/session/driver/src/conv.rs:22-39`). No second socket request occurs on the normal Session path.

### 6. Runtime preflight and launch preparation

The Runtime domain path:

1. checks SessionId and target conflicts;
2. selects the Claude or Codex launcher;
3. creates a `LaunchSpec`;
4. applies host or Docker backend preparation;
5. begins a coordinated spawn;
6. launches a shim;
7. waits up to ten seconds for `ShimReady`;
8. records the Running lifecycle and starts the exit watcher.

Evidence: `internal/runtime/daemon/src/api.rs:74-115`.

Runtime recognizes this as Session backed because Transaction A already created a matching `Forking` lifecycle row. Raw Runtime spawn has no such row, so Runtime inserts its own `Forking` row (`internal/runtime/daemon/src/server/spawn.rs:33-70`). This database shape discriminator controls whether Runtime appends the Running event itself (`internal/runtime/daemon/src/api.rs:97-99`).

### 7. Shim handoff and runtime process

`SpawnCoordinator` keeps the prepared `LaunchSpec` in a pending map and registers a `ShimReady` channel (`internal/runtime/daemon/src/server/spawn.rs:57-65`, `internal/runtime/daemon/src/server/spawn.rs:121-153`). The backend launches only the shim (`internal/runtime/daemon/src/backend.rs:44-53`, `internal/runtime/daemon/src/backend.rs:96-99`).

For tmux, Runtime respawns the selected pane with `lilo __shim`. For headless mode, it starts the shim as a child and copies stdout and stderr to Session log files (`internal/runtime/daemon/src/shim_socket.rs:38-54`, `internal/runtime/daemon/src/shim_socket.rs:67-108`). The bootstrap environment contains only `LILO_SOCKET_PATH`; the real launch environment arrives through the socket handoff (`internal/runtime/daemon/src/shim_socket.rs:139-156`).

The shim requests `ShimLaunch`, receives and removes the pending `LaunchSpec`, clears inherited environment, starts the real agent process, reports `ShimReady`, waits for exit, and reports `ShimExit` (`internal/runtime/app/src/cli/shim.rs:35-75`, `internal/runtime/daemon/src/handler.rs:180-190`).

### 8. Runtime Running evidence

After `ShimReady`, Runtime updates `runtime_lifecycle`, starts an exit watcher, and returns a Running lifecycle plus event. Session backed flow leaves the event unappended until Session commits (`internal/runtime/daemon/src/server/spawn.rs:174-213`).

### 9. Transaction B: evidence after side effect

Session revalidates that the namespace still exists, begins another transaction, and performs these operations together:

1. inserts the hydrated `session_sessions` row;
2. updates the matching Runtime lifecycle to Running;
3. resolves the spawn intent.

If commit fails, Session asks Runtime to terminate the orphan and aborts the intent. After a successful commit, Session uses the concrete `RuntimeService` to append the Running event (`internal/session/daemon/src/handler/spawn.rs:138-210`).

### 10. Response and background convergence

Session returns `RpcResponse::Spawned`; the CLI prints the hydrated Session (`internal/session/daemon/src/handler/spawn.rs:91-93`, `internal/session/app/src/cli/run.rs:88-98`).

The Session Runtime event task long polls Runtime through `RuntimePort`, atomically applies events plus cursor to Session storage, and falls back to Runtime status when its cursor expires (`internal/session/daemon/src/events.rs:33-68`, `internal/session/daemon/src/events.rs:76-108`). Startup also reconciles unresolved spawn intents before the listener begins serving (`internal/session/app/src/compose.rs:128-137`).

## Raw Runtime spawn contrast

`lilo runtime spawn` travels through `RuntimeClient` and `LilodRpc::Runtime`. The Runtime wire handler extracts the peer principal, authorizes Spawn, records audit, and calls the same `spawn_domain` helper (`crates/lilo-rm-client/src/lib.rs:72-80`, `internal/runtime/daemon/src/handler.rs:127-190`, `internal/runtime/daemon/src/identity.rs:36-60`).

Because no Session transaction inserted a `Forking` lifecycle first, Runtime owns the full lifecycle write and event append. No `session_sessions` or `session_spawn_intents` row is created. This matches the documented diagnostic exception (`README.md:80-82`, `docs/architecture/system.md:83-85`).

## Persistence and evidence ownership

The unified migration creates ten tables: Identity audit, Session records, namespaces, messages, deliveries, labels, event cursor, spawn intents, Runtime lifecycle, and Runtime metadata (`internal/db/migrations/0001_unified_schema.sql:9-153`). `LiloDb::open_postgres` creates one pool and runs this migration (`internal/db/src/lib.rs:42-61`).

| Owner | Durable state | Evidence |
| --- | --- | --- |
| Identity | `identity_audit` | `internal/db/migrations/0001_unified_schema.sql:9-28` |
| Session | `session_sessions`, namespaces, labels, mail, event cursor, spawn intents | `internal/db/migrations/0001_unified_schema.sql:30-128` |
| Runtime | `runtime_lifecycle`, `runtime_metadata` | `internal/db/migrations/0001_unified_schema.sql:130-153` |
| Runtime event stream | Append only JSONL under `LILO_HOME` with recovery, in memory index, cursor, and compaction | `internal/runtime/daemon/src/event_log.rs:100-128`, `internal/runtime/daemon/src/event_log.rs:130-169` |

The shared join key is `SessionId`. Session mints it before Runtime exists; Session intent, Runtime lifecycle, Identity audit, mail relationships, and future Transport records use the same typed value (`docs/architecture/system.md:87-99`).

## Identity boundary

Identity is a library layer in both Session and Runtime. The composed listener derives `Principal` from Unix peer credentials (`internal/session/app/src/compose.rs:226-246`). Session has an `IdentityPort` so tests or later implementations can replace the concrete client (`internal/session/daemon/src/identity_client.rs:12-64`). Runtime currently uses `IdentityClient` directly.

Current authorization is intentionally local and minimal. `AuditDecision::evaluate_local` allows the daemon owner's UID and denies other or unknown principals (`crates/lilo-im-core/src/audit.rs:26-36`). The stub authorizer records the decision, returns role `admin`, and exposes no capability list (`crates/lilo-im-stub/src/lib.rs:43-61`). Rich RBAC and service account behavior remain bounded context direction rather than implemented policy.

Session Spawn authorization is especially strong: the audit decision shares Transaction A with the pending intent and Runtime `Forking` row (`internal/session/daemon/src/handler/spawn.rs:102-135`). Raw Runtime Spawn records authorization in a Runtime store transaction before proceeding (`internal/runtime/daemon/src/identity.rs:36-60`).

## Implemented state versus target design

| Context or seam | Implemented at HEAD | Target direction |
| --- | --- | --- |
| Identity | Same UID authorization, Postgres audit, Session port, Runtime direct client | Service identity and richer RBAC shape. No standalone command until real verbs exist. |
| Session | Full CLI and MCP surface, Session records, selectors, labels, mail, namespace, spawn intent reconciliation | Keeps logical intent and capture preparation; submits opaque launch to Schedule. |
| Runtime | Process launch, shim, host and Docker backend, tmux, lifecycle Postgres store, JSONL events, reconciliation | Executes Schedule selected topology and opaque launch payload without provider semantics. |
| Schedule | No crate, schema, daemon, or command | Sole placement authority, stable occupant bindings, desired topology, restart policy reconciliation (`docs/architecture/schedule.md:3-45`, `docs/architecture/schedule.md:95-119`). |
| Transport | No crate, helper, capture lease, request store, or command | Provider wire capture, exact bytes, interpretation, authorized transformation, fidelity evidence (`docs/architecture/transport.md:3-46`). |
| Canvas and Desktop | No app implementation | One product surface over stable `lilod` read and command contracts, with no direct storage reads (`docs/architecture/canvas.md:3-42`). |
| Placement flow | `Canvas` absent; `lilo -> Session -> InProcessRuntime -> Runtime` | `Canvas or lilo -> Session -> Schedule -> Runtime`, plus Session to Transport (`docs/architecture/system.md:20-66`). |
| Launch attachment | Session and Runtime `SpawnRequest` contain explicit Runtime fields only | Opaque capture lease and launch additions carried through Schedule and Runtime (`internal/session/core/src/proto/spawn.rs:11-38`, `crates/lilo-rm-core/src/types/spawn.rs:206-223`, `docs/architecture/system.md:63-66`). |
| Stable topology identity | SessionId plus current tmux address on lifecycle | Occupant token to Schedule pane ID to live tmux ID; positional address becomes display data (`docs/architecture/schedule.md:50-68`). |

The target first proof is deliberately narrow: `lilo run claude`, capture the first request, render original and interpreted evidence, edit one tool description by name, validate and forward, receive the response, and show audit evidence (`docs/architecture/system.md:111-126`, `docs/architecture/canvas.md:98-110`). None of this Transport or Canvas loop exists in the current source.

## Boundaries that are working well

1. **One socket, typed substrate envelope.** Session and Runtime share transport mechanics while retaining separate protocol enums (`internal/wire/src/lib.rs:1-8`).
2. **One domain path for Runtime behavior.** Raw wire commands and in process Session calls converge on the same Runtime domain helpers (`internal/runtime/daemon/src/api.rs:21-72`, `internal/runtime/daemon/src/handler.rs:127-190`).
3. **Intent around process side effects.** The two transaction spawn sequence makes crash recovery explicit and gives startup a durable pending set (`internal/session/daemon/src/handler/spawn.rs:96-210`, `internal/session/daemon/src/handler/spawn.rs:257-324`).
4. **Launcher and backend are separate choices.** Runtime kind selects command construction; isolation selects host or Docker execution (`internal/runtime/daemon/src/api.rs:81-85`, `internal/runtime/daemon/src/backend.rs:33-53`).
5. **Shim limits inherited authority.** Only the socket path crosses bootstrap; the authoritative environment and working directory arrive in `LaunchSpec` after the shim connects (`internal/runtime/daemon/src/shim_socket.rs:139-156`, `internal/runtime/app/src/cli/shim.rs:35-48`).
6. **Stores own persistence mechanics.** Daemons orchestrate transactions while store crates own SQL and codecs. Session and Runtime stores depend inward on shared database and core types.
7. **Event tail supports convergence.** Cursor based long polling, status fallback, and startup intent recovery cover incremental and discontinuous evidence (`internal/session/daemon/src/events.rs:33-108`).

## Gaps and surprising coupling

### 1. Architecture docs describe a socket Session to Runtime path that current composition does not use

The Session diagram shows `Driver -> RuntimeClient -> RuntimeDaemon` and a separate Session daemon socket (`docs/architecture/session.md:79-120`). Runtime's stable flow says current Session spawn passes through `RuntimeRpc::Spawn` (`docs/architecture/runtime.md:132-139`). Current factories construct `InProcessRuntime`, which calls `RuntimeService::spawn` directly (`internal/session/daemon/src/service.rs:69-85`, `internal/session/driver/src/in_process.rs:49-65`).

`RtmdDriver` remains publicly exported and functional (`internal/session/driver/src/lib.rs:3-21`, `internal/session/driver/src/rtmd.rs:24-55`), but fmm found no production construction outside that reexport. The docs should label it as an alternate or conformance adapter, then show the composed path as current.

### 2. Session keeps both the Runtime abstraction and its concrete implementation

`DaemonState` stores `Arc<dyn RuntimePort>` and `Arc<RuntimeService>` together (`internal/session/daemon/src/handler/state.rs:17-29`). Normal spawn, capture, terminate, nudge, status, and event polling use the port. Successful Session commit calls the concrete service directly to append the Running event (`internal/session/daemon/src/handler/spawn.rs:205-208`).

This coupling is the main obstacle to replacing direct Runtime placement with Schedule. A target design needs one owner for post commit publication. Options include a port operation, a Session owned outbox consumed by Runtime, or a Schedule mediated commit protocol. The choice should preserve the current rule that no Running event becomes visible before the Session row commits.

### 3. The Runtime port erases typed SessionId at its boundary

`RuntimePort` accepts `&str` for spawn, capture, terminate, and nudge (`internal/session/driver/src/port.rs:18-50`). `InProcessRuntime` immediately reparses it into `SessionId` (`internal/session/driver/src/in_process.rs:49-65`, `internal/session/driver/src/in_process.rs:83-100`). The rest of the workspace uses the typed UUIDv4 family.

This is an imported socket era shape. Keeping `SessionId` typed through the internal port would remove repeated parsing and make illegal IDs unrepresentable. The socket adapter can stringify at its own boundary.

### 4. Session logs read Runtime produced files directly

Capture correctly goes through Runtime. Logs load `transcript_path` from the Session row and call `fs::read` inside Session daemon (`internal/session/daemon/src/polish.rs:15-43`, `internal/session/daemon/src/polish.rs:137-146`). Runtime created those headless log files (`internal/runtime/daemon/src/shim_socket.rs:67-108`).

This is a storage level dependency across the Session and Runtime ownership boundary. It will complicate a future distributed mapping and Canvas read model. A Runtime log read port or a composed read service would keep Session and Canvas away from Runtime file layout.

### 5. No opaque launch payload seam exists yet

Both current spawn contracts enumerate known fields. Neither contains an opaque payload, capture lease, or transport additions (`internal/session/core/src/proto/spawn.rs:11-38`, `crates/lilo-rm-core/src/types/spawn.rs:206-223`). The target requires Session to attach Transport capture and later pass the same opaque value through Schedule (`docs/architecture/system.md:63-66`, `docs/architecture/transport.md:32-46`).

The first Transport slice must introduce this seam once, then thread it through the direct path in a form Schedule can later forward unchanged. A provider specific field on Runtime or Schedule would violate the documented boundary.

### 6. Session state documentation overstates a persisted Spawning row

The Session document says creation inserts a spawning Session row before calling Runtime (`docs/architecture/session.md:149-154`). Current Transaction A inserts a pending intent and a Runtime `Forking` row. The draft Session is already `Running` and is inserted only in Transaction B after Runtime reports ready (`internal/session/daemon/src/handler/spawn.rs:44-68`, `internal/session/daemon/src/handler/spawn.rs:120-135`, `internal/session/daemon/src/handler/spawn.rs:138-181`).

`SessionState::Spawning` exists in the domain enum (`internal/session/core/src/session.rs:17-22`), but the implemented launch flow represents pending state through `session_spawn_intents`. The documentation should describe the intent row accurately or the code should persist an actual Spawning Session if that visibility is required.

### 7. Event deduplication may conflict with future same SessionId restarts

Runtime event deduplication keys only on `(SessionId, event kind)` (`internal/runtime/daemon/src/event_log.rs:53-81`, `internal/runtime/daemon/src/event_log.rs:135-147`). The target Schedule design keeps a logical SessionId across resume and may emit multiple Running or terminal transitions over time (`docs/architecture/system.md:101-109`, `docs/architecture/schedule.md:112-119`).

This is a target compatibility risk, not a current bug. Schedule activation needs an attempt, generation, or occupant identity in Runtime event semantics before same SessionId restarts can be represented without suppression.

### 8. Session backed classification depends on a shared database shape

Runtime infers Session ownership by finding a matching `Forking` lifecycle that Session inserted in Transaction A (`internal/runtime/daemon/src/server/spawn.rs:33-56`). This elegantly avoids an untrusted `session_backed` request flag and preserves raw Runtime behavior. It also couples the two contexts to one Postgres transaction and a precise lifecycle precondition.

That coupling is valid for v1 and matches the local first strategy. Schedule activation must deliberately become the writer of the corresponding precondition or replace the inference with another trustworthy owner signal. Preserving both writers would create two placement paths.

### 9. Identity policy is much smaller than the bounded context description

Current authorization is owner UID equality with an `admin` result and no capabilities (`crates/lilo-im-core/src/audit.rs:26-36`, `crates/lilo-im-stub/src/lib.rs:43-61`). The docs describe service account identity and RBAC shape (`README.md:22-26`). The current implementation provides the audit and admission seam, while resource policy and roles remain future work.

## Open questions

1. Which component owns post commit Runtime event publication after Schedule mediates placement?
2. Should `RuntimePort` move to typed `SessionId` before Transport or Schedule work begins?
3. Should `lilo logs` become a Runtime read operation, or should a new composed read model own log bytes?
4. Is `RtmdDriver` retained only for parity and future v2 transport, or should production exports be narrowed until needed?
5. Which attempt or generation identity will distinguish repeated lifecycle transitions for one resumed SessionId?
6. What exact opaque launch type can carry Transport capture without importing provider semantics into Session core, Schedule, or Runtime?
7. Which Postgres context owns Transport capture records and the Session joined transcript read model? The Transport document leaves this open (`docs/architecture/transport.md:67-74`).

## Verification

This review verified source definitions and their callers at the pinned HEAD. The main evidence chain was:

1. fmm project topology, outlines, symbol reads, dependency graphs, glossary call sites, and runtime dependency cycle scan.
2. Root and package Cargo manifests plus `cargo metadata --no-deps`.
3. CLI entrypoint, Session app request builders, unified daemon composition, Session dispatch and spawn transactions, Runtime port adapters, Runtime domain API, shim handoff, stores, Identity, and event tail.
4. Current architecture documents for System, Session, Runtime, Schedule, Transport, and Canvas.
5. Final Git status remained clean on `main` at `753e2ee91e3d41ae74dbe1cae3ec3ee6797e61da`.

No build or test command ran. The task required read only architecture analysis, and Cargo build or test would write target artifacts. Structural acceptance was proved through current definitions, direct call sites, and repository metadata.
