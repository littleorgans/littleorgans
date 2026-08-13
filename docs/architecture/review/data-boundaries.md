# lilo architecture review: domain models, ownership, persistence, boundaries

**HEAD:** `753e2ee91e3d41ae74dbe1cae3ec3ee6797e61da` (`docs(architecture): define Canvas, Schedule, and Transport boundaries`)
**Scope:** current repository, read only. Evidence from authored source, migrations, tests, and architecture docs at this SHA.
**Workspace:** 271 source files, 37,374 LOC (`internal/` 26,988; `crates/` 9,948). Session 15,654; Runtime 10,453; Identity service 93.

This review answers: who owns which facts, where they live, how they move, and where the current code violates or defers the documented context map.

---

## 1. Sources of truth

### 1.1 Documented product map

`docs/architecture/system.md` is the governing bounded-context map. Five contexts are intended; three are implemented.

| Context | Owns | Does not own | Implementation at HEAD |
| --- | --- | --- | --- |
| Identity | authorization, audit, service identity, RBAC shape | session meaning, placement, process execution | Library crates + stub authorizer. No daemon, no CLI namespace. |
| Session | logical sessions, operator verbs, intent, mail, nudge, labels | topology, provider payloads, process internals | Full five-subdir stack. Composes `lilod`. |
| Schedule | placement, desired topology, occupant bindings, restart policy | agent meaning, launch internals, provider traffic | **Reserved.** No crate, schema, or command namespace. `docs/architecture/schedule.md:1-4`. |
| Runtime | process launch, shim, platform execution, lifecycle evidence | placement, session meaning, payload policy | Full published + internal stack. |
| Transport | provider wire capture, payload interpretation, overlays, fidelity | authorization, placement, process selection | **No monorepo implementation.** `docs/architecture/transport.md:1-4`. |

Canvas/Desktop is one product surface, not a bounded context. **No Canvas/Desktop code.** `docs/architecture/canvas.md:1-8`.

Target dependency direction (`docs/architecture/system.md:53-61`):

```text
Canvas or lilo -> Session -> Schedule -> Runtime
                      |
                      +-> Transport -> provider
Identity authorizes Session, Schedule, Runtime, and Transport service actions.
```

Current implementation direction:

```text
lilo -> Session (or diagnostic Runtime CLI)
Session -> Identity (library)
Session -> Runtime (direct driver + shared-tx lifecycle write)
lilod = SessionService + RuntimeService on one Unix socket (LilodRpc)
```

The direct Session-to-Runtime call is documented as interim (`system.md:22-28`, `session.md:34-39`, `runtime.md:29-34`). It is the only session-backed launch path in code.

### 1.2 Join key

`SessionId` is the platform join key across Session, Runtime, Transport, and Canvas (`system.md:87-90`). The runtime spawn id is a `SessionId`, not a separate type. Events have no id field. No `EventId` or `NamespaceId` exists. Schedule ids are deferred until a stored field needs one (`schedule.md:66-68`).

### 1.3 Authored contract crates vs stores vs daemons

| Layer | Session | Runtime | Identity |
| --- | --- | --- | --- |
| Published contract | none (internal `lilo-session-core`) | `lilo-rm-core` | `lilo-im-core` |
| Published client | none | `lilo-rm-client` | none |
| Internal store | `lilo-session-store` | `lilo-runtime-store` | published `lilo-im-store` |
| Internal daemon | `lilo-session-daemon` | `lilo-runtime-daemon` | none |
| App / CLI | `lilo-session-app` + unified `crates/lilo` | `lilo-runtime-app` (diagnostic) | none |
| Driver / port | `lilo-session-driver`, `lilo-port` | `lilo-runtime-launchers` | `lilo-identity-service` (client wrapper) |
| Shared persistence | `lilo-db` + `internal/db/migrations/0001_unified_schema.sql` | same | same |
| Shared paths | `lilo-paths` | `lilo-paths` | `lilo-paths` (URL only) |
| Platform primitives | `lilo-sys` (peer creds) | `lilo-sys` (process, signal, ipc) | `lilo-sys` (peer creds) |

---

## 2. Ownership table (data)

| Fact | Owner | Type / table | Writer | Readers | Persistence |
| --- | --- | --- | --- | --- | --- |
| Typed ids (`SessionId`, `MessageId`, `IntentId`, `AuditId`) | `lilo-common` | `define_id!` newtypes | constructors (`::new()` = UUIDv4) | all contexts | wire/disk = 36-char string; sqlx Type is `uuid` but stores bind `TEXT` |
| Operator home, socket, logs, events path | `lilo-paths` | `LiloPaths` | env / `LILO_HOME` | daemons, CLI, tests | filesystem under `~/.lilo/` |
| Env name registry | `lilo-paths::env` | consts | n/a | `scripts/check-env.sh`, spawn env | process env |
| DB URL / pool | `lilo-db` | `DbConfig`, `LiloDb` | env over `settings.toml` | all stores | Postgres |
| Unified schema | `lilo-db` migrator | 10 tables | `LiloDb::open_postgres` | stores | Postgres |
| Principal | Identity | `Principal::{Local,Unknown}` | peer creds at socket accept | session/runtime handlers | serialized into `identity_audit.principal` |
| Authz decision | Identity | `AuditDecision`, `Action`, `ResourceSpec` | `IdentityClient` / `StubAuthorizer` | session spawn tx | `identity_audit` |
| Logical session row | Session | `Session` / `session_sessions` | session store + event tail | CLI `get`/`list`, mail, polish | Postgres |
| Session labels | Session | `session_labels` | session store | selectors | Postgres |
| Namespaces | Session | `Namespace` / `session_namespaces` | session store | spawn, list, delete cascade | Postgres |
| Mail | Session | `messages` + `message_deliveries` | session store | mail RPC | Postgres |
| Spawn intent | Session | `session_spawn_intents` | session daemon spawn | recovery | Postgres (transient) |
| Runtime event cursor (session copy) | Session | `session_event_cursor` | session store event task | reconciliation | Postgres |
| Runtime lifecycle row | Runtime | `Lifecycle` / `runtime_lifecycle` | runtime store; **also session spawn Tx A** | status, reconcile, session complete | Postgres |
| Runtime metadata | Runtime | `runtime_metadata` | runtime store | last probe sweep | Postgres |
| Runtime events | Runtime daemon | `RuntimeEvent` JSONL | daemon `append_event` | session tail, `lilo runtime events` | `$LILO_HOME/data/events/runtime.jsonl` |
| Tmux pane snapshot | Runtime | `CaptureRequest` / `PaneSnapshot` | runtime daemon (`tmux capture-pane`) | session `lilo capture` | ephemeral; not a Transport record |
| Isolation / mounts / shell resume | Runtime contract | `IsolationPolicy`, `MountSpec`, `ShellResume` | parsed at CLI / session spawn | runtime launch | copied into spawn JSON |
| Restart policy / topology / occupant binding | Schedule | none | none | none | **gap** |
| Provider bytes / overlays / capture lease | Transport | none | none | none | **gap** |
| Canvas report model | Canvas | none | none | none | **gap** |

---

## 3. Typed ID family

Source: `crates/lilo-common/src/id.rs`.

```22:96:crates/lilo-common/src/id.rs
macro_rules! define_id {
    ($name:ident) => {
        #[derive(..., ::serde::Serialize, ::serde::Deserialize)]
        #[serde(transparent)]
        #[cfg_attr(feature = "sqlx", derive(sqlx::Type), sqlx(transparent))]
        pub struct $name(::uuid::Uuid);
        impl $name {
            pub fn new() -> Self { Self(::uuid::Uuid::new_v4()) }
            pub fn short(&self) -> String { self.short_with(|_| true) }
            // Display/FromStr = full 36-char hyphenated UUID
        }
    };
}
define_id!(SessionId);
define_id!(MessageId);
define_id!(IntentId);
define_id!(AuditId);
```

| Property | Evidence |
| --- | --- |
| Generation | `new()` calls `Uuid::new_v4()`. Test `new_generates_uuid_v4` asserts version 4 (`id.rs:132-136`). |
| Display / parse | Full 36-char. Test `all_id_types_roundtrip_full_uuid_display_and_parse` (`id.rs:124-129`). |
| Serde | Transparent bare UUID string (`id.rs:139-148`). |
| sqlx | `#[sqlx(transparent)]` over `uuid::Uuid` (native `uuid` column). Test `sqlx_postgres_insert_select_roundtrip` uses `CREATE TEMP TABLE ids (id uuid)` (`id.rs:179-207`). |
| Short form | Floor 7 hex chars (`MIN_SHORT_PREFIX_LEN = 7`, `id.rs:4`). `short()` is human surface only. |
| Selector prefix | Session store uses `MIN_SELECTOR_PREFIX_LEN = 4` (`selector/types.rs:8`), **not** the 7-char short floor. |

**Uses:**

- `SessionId`: session row PK, runtime lifecycle PK, mail recipient, audit `session_ref`, spawn env `LILO_AGENT_SESSION_ID`, join key.
- `MessageId`: mail `messages.message_id`.
- `IntentId`: spawn intent `operation_id` (`spawn_intents.rs:133`). Production mint is `IntentId::new()` (`handler/spawn.rs:65`). Tests still construct `IntentId::from_uuid(Uuid::now_v7())` (`spawn_intents.rs:492`, `handler/spawn/tests.rs:350`, `daemon/src/service.rs:200`).
- `AuditId`: `AuditRow.id` (`lilo-im-core/src/audit.rs:41`).

No `EventId`. Runtime events are cursor-addressed (`EventCursor = u64`, `lilo-rm-core/src/proto.rs:23`). No `NamespaceId`; namespaces are validated slugs.

**Storage mismatch:** sqlx Type is native `uuid`, but the unified schema stores ids as `TEXT` (`0001_unified_schema.sql:10,31,64,116,131`). Store codecs bind `session_id.to_string()` (`lifecycle.rs:106`, `sessions.rs:62-67,85-86`) and parse `String` back (`sessions.rs:425`, `lifecycle/codec.rs:17,64`). The typed-id sqlx impl is unused on the real tables.

---

## 4. Domain models by context

### 4.1 Session

**Layering** (`docs/architecture/session.md:199-208`, crate layout 121 files / 15,654 LOC):

| Crate | Role |
| --- | --- |
| `lilo-session-core` | Internal contract: RPC, Session, Selector, mail, labels, namespaces, tool contracts. Re-exports `IsolationPolicy`/`MountSpec` from `lilo-rm-core` (`lib.rs:25`). |
| `lilo-session-store` | Postgres: sessions, namespaces, mail, labels, event cursor, spawn intents. |
| `lilo-session-driver` | Runtime bridge: `runtime_spawn_request` conversion, capture/nudge/kill mapping, `RuntimePort`. |
| `lilo-session-daemon` | Socket serve, authorize, persist intent, drive runtime, persist evidence, reconcile. |
| `lilo-session-app` | CLI/MCP + **composed `lilod`** (`compose.rs`). |

**`Session` / `SessionState`** (`internal/session/core/src/session.rs:15-89`):

```text
Spawning | Running | Terminated | Lost { evidence: LostEvidence }
```

SQL names: `SPAWNING` / `RUNNING` / `TERMINATED` / `LOST` (`session.rs:40-63`). `LostEvidence` is re-exported from `lilo-rm-core` (`session.rs:13`).

Fields: `id: SessionId`, `runtime: RuntimeKind` (session-local Claude/Codex only), `role`, `workspace`, `namespace`, `dir`, `labels`, `state`, `runtime_pid: u32` (NOT NULL, 0 while drafting), `runtime_session`, `transcript_path`, `tmux_pane: Option<String>` (untyped), `agent_config`, timestamps, `exit_code`.

**`Selector`** (`selector/types.rs:13-38`): `Id`, `Prefix`, `Label`, `Namespace`, `Dir`, `And`, `Role`, `All`. Parser (`selector/parser.rs:14-66`) accepts bare UUID, `id:`, `role:`, `namespace:`, `dir:`, `label:`, or hex/hyphen prefix of length >= 4. `workspace:` is explicitly rejected (`parser.rs:35-40`). Prefix resolution is store-owned: `WHERE id LIKE $1 || '%'` with ambiguity error (`sessions.rs:163-182`).

**`Namespace`** (`namespace.rs:7-56`): DNS-label slug, max 63, reserved prefix `sm-`, `default` cannot be created. Seeded in schema (`0001_unified_schema.sql:60-61`).

**Mail** (`mail.rs`):

- Status: `unread | read | undeliverable` (matches SQL CHECK).
- Intent: `request | result | inform | receipt`. Client cannot send `receipt` (`mail.rs:64-71`).
- `SenderRef`: Session / Operator / System (`mail.rs:181-186`).
- `MessageId` + `SessionId` recipients.
- Notify modes `wait`/`steer` convert to runtime `NudgeMode` (`mail.rs:128-134`). Mail is durable; nudge is ephemeral (`session.md:73-75`).

**Spawn intent** (`store/src/postgres/spawn_intents.rs:37-41,115-125`):

```text
pending -> resolved | aborted
```

Stores serialized **runtime** `SpawnRequest` plus `SessionDraft`. Timestamps are epoch-millis `BIGINT` by design (schema comment `0001_unified_schema.sql:113-114`), unlike every other table's `TIMESTAMPTZ`.

**Session RPC** (`proto/rpc.rs:23-46`): `Spawn`, `List`, namespace CRUD, `Delete`, mail family, `Nudge`, `Label`, `Logs`, `Capture`, `Doctor`, `Wait`, `CallerContext`, `McpBridge`, `Shutdown`.

**Session `SpawnRequest`** (`proto/spawn.rs:11-38`): no `session_id`. Includes role, workspace, dir, namespace, labels, agent_config, plus runtime-shared isolation/image/env/mounts/shell_resume/force. Default target `"headless"`.

### 4.2 Runtime

**Layering** (`docs/architecture/runtime.md:165-176`, 74 files / 10,453 LOC):

| Crate | Role |
| --- | --- |
| `lilo-rm-core` | Published protocol. `RuntimeRpc`/`RuntimeResponse`, `Lifecycle`, spawn, capture, MCP, version. `RUNTIME_PROTOCOL_VERSION = "0.8"` (`version.rs:8`). |
| `lilo-rm-client` | Typed Unix-socket client + event watcher. |
| `lilo-runtime-store` | `runtime_lifecycle` + `runtime_metadata`. |
| `lilo-runtime-daemon` | Dispatch, shim protocol, Docker, reconcile, JSONL events, `RuntimeService::build`. |
| `lilo-runtime-launchers` | Command resolution (claude/codex). |
| `lilo-runtime-app` | Diagnostic `lilo runtime ...` + shim entry. |
| `lilo-sys` | OS process/signal/ipc/creds. tmux mapping stays daemon-internal (`runtime.md:175`). |

**`Lifecycle` / `LifecycleState`** (`types/lifecycle.rs:13-18,53-65`):

```text
Forking -> Running -> Exited(RuntimeExit)
                   -> Lost(LostEvidence)
Forking -> Lost
```

Transitions are methods on `Lifecycle`: `forking`, `mark_running`, `mark_exited`, `mark_lost` (`lifecycle.rs:68-118`). Tests: `lifecycle_transitions_from_forking_to_running_to_exited` (`221-233`), `..._to_lost` (`236-245`), `..._running_to_lost` (`248-258`).

SQL codec stores `Forking|Running|Exited|Lost` (`lifecycle/codec.rs:10-13`). This is a **different vocabulary** from session `SPAWNING|RUNNING|TERMINATED|LOST`.

**`LostEvidence`**: `ShimDiedBeforeReport | PidNotAlive | PidReuseDetected` (`lifecycle.rs:146-150`). Shared with Session.

**Runtime `SpawnRequest`** (`types/spawn.rs:206-223`): **includes `session_id`**. Target is typed `SpawnTarget::{Tmux(TmuxAddress), Headless}`. No role/workspace/namespace/labels.

**`TmuxAddress`** (`spawn.rs:12-16`): `{session, window, pane}` with `FromStr`/`Display`. Session stores the same fact as `Option<String>`.

**`RuntimeRpc`** (`proto.rs:88-137`): Spawn, ValidateTarget, Kill, KillByPid, Nudge, Capture, Status, Version, Watchers, WaitWatchers, Doctor, Events, Stop, McpBridge, ShimLaunch, ShimReady, ShimExit.

**Protocol version / capabilities** (`version.rs:8-24`): `0.8` plus 13 capabilities including cursor events, spawn conflicts, mounts, nudge wait timeout. Tests lock the advertised set (`version.rs:154-167`).

**Capture (runtime, not Transport):** `CaptureRequest { session_id, scrollback_lines }` (`capture.rs:42-46`). Result is a tmux pane snapshot or `CaptureError`. This is `lilo capture` / Runtime's pane verb (`session.md` polish; Agents.md: `lilo capture` remains Runtime's tmux pane capture). It is **not** a Transport capture lease.

**Events:** appended in observation order to JSONL (`LiloPaths::events_log_path` = `$LILO_HOME/data/events/runtime.jsonl`, `lilo.rs:107-108`). Cursor type `u64`. Retention constants: 7 days / 10_000 events (`proto.rs:25-26`). Expired cursor returns `CursorExpired { oldest }`; clients must reconcile via Status (`runtime.md:155-158`).

### 4.3 Identity

**Layering** (tiny):

| Crate | Role | LOC |
| --- | --- | --- |
| `lilo-im-core` | `Authorizer`, `Principal`, `Action`, `ResourceSpec`, `AuditRow`, peer creds | 451 |
| `lilo-im-store` | `identity_audit` via `AuditStore` | 288 |
| `lilo-im-stub` | `StubAuthorizer` | 62 |
| `lilo-identity-service` | `IdentityClient` wrapping stub + store | 93 |

**`Principal`** (`types.rs:13-19`): `Local(u32)` or `Unknown { kind, raw }` for forward-compatible rows. Wire tag `kind: "Local"` (`types.rs:209-213`).

**`Action`** (`types.rs:147-160`): Spawn, Kill, List, Read, Logs, MailSend, MailRead, Nudge, Link, Doctor, Daemon, ShimCallback.

**`ResourceSpec`** (`types.rs:171-187`): optional workspace, role, runtime (`im-core` `RuntimeKind` with `Other`), `session_id`, labels.

**v1 authz rule** (`audit.rs:16-36`): `Principal::Local` matching daemon `local_uid` => Allow; other Local => Deny `"non-local uid"`; Unknown => Deny `"unknown principal"`. Never yields `Error`. Both `StubAuthorizer` (`lilo-im-stub/src/lib.rs:49`) and `IdentityClient::authorize_in_tx` (`identity/service/src/client.rs:59`) call this one function.

Crate docs disagree with the code:

- `lilo-im-core/src/lib.rs:1-3`: "Authorization is NOT enforced in v1".
- `lilo-im-stub/src/lib.rs:1-2`: "audits every decision without enforcement".

The uid check **is** enforced. Denied principals get `AuthzError::UnknownPrincipal` (`stub:59-60`, `client.rs:72`). There is no RBAC, no service accounts, no capability expansion (`Authorized.role` is hardcoded `"admin"`, `stub:55`).

**Peer creds** (`peer_creds.rs:4-13`): `lilo_sys::creds::peer_cred` on the IPC stream -> `Principal::local(uid)`. Extracted at `lilod` accept (`compose.rs:226`), session daemon accept (`daemon/src/server.rs:109`), and runtime handler (`runtime/daemon/src/handler.rs:31`). `local_uid` is the daemon process uid (`lilo_sys::creds::current_uid()`).

**Consumption (library layer, both daemons):**

- Session `authz_plan` is exhaustive over `SessionRpc` (no `_` arm) (`handler/authz.rs:13-49`). Door verbs authorize `ResourceSpec::default()` before dispatch: List, NamespaceList, NamespaceGet, Wait, MailCheck, MailStopCheck, **NamespaceCreate**. Downstream verbs resolve a session first (spawn, delete, mail send/read, nudge, label, logs, capture, doctor, shutdown).
- Runtime authorizes every `RuntimeRpc` before domain work (`runtime/daemon/src/identity.rs:76-93`). Shim launch/ready/exit use `Action::ShimCallback`.
- `IdentityClient::authorize` maps `Authorized` to `()` (`identity/service/src/client.rs:40-49`). Role and capabilities never reach callers.
- Deny is always `AuthzError::UnknownPrincipal` even when the principal is a known Local uid with reason `"non-local uid"`. `AuthzError::Unauthorized` exists and is unused.
- Action vocabulary is overloaded: `NamespaceCreate` and `NamespaceDelete` use `Action::Kill` (`authz.rs:31-33`, `namespace.rs:93`). Labels use `Action::Link`. Session capture uses `Action::Read` (`handler/sessions.rs:42-47`); runtime capture uses `Action::Logs` (`identity.rs:82-84`).
- List / wait / namespace-get audit an empty resource, so the row cannot say which sessions were listed.

**No CLI namespace.** Guarded: `assert!(!help.contains("identity"))` (`crates/lilo/src/cli.rs:336-343`). `whoami` / `can-i` / audit verbs do not exist. Closest operator signal is `lilo doctor` counting `identity_audit` rows. Human `short()` rendering is `ShortSessionIdSet` in session CLI output (`session/app/src/cli/output.rs:20-24`); JSON stays full UUID.

### 4.4 Shared platform

**`lilo-db`**: one pool, one migrator, `EXPECTED_TABLES` of 10 names (`internal/db/src/lib.rs:162-173`). `open_postgres_runs_migrations_and_creates_unified_schema` (`184-201`) is the schema acceptance test. Pool max 5, connect timeout 5s (`config.rs:10-12`). Refuses to guess a host (`config.rs:14`). Env wins over `$LILO_HOME/settings.toml` (`config.rs:28-36`, test `resolve_prefers_env_over_settings` at `71+`).

**`lilo-paths`**: `~/.lilo/` tree: config, run (`lilod.sock`, `lilod.pid`), data, logs, cache, tmp, `data/events/runtime.jsonl`. `LILO_SOCKET_PATH` overrides only the socket (`lilo.rs:95-96`, test `socket_override_wins_without_moving_other_paths`). Settings file is `[database]` only, `deny_unknown_fields` (`settings.rs:18-33`).

**`lilo-wire`**: 8 lines. `LilodRpc::{Session(SessionRpc), Runtime(RuntimeRpc)}` (`internal/wire/src/lib.rs:5-8`). Framing is JSON lines from `lilo-rm-core` (`read_optional_json_line` / `write_json_line` in compose).

**`lilo-port`**: `PortError<F> = Fault(F) | Opaque(OpaqueFault)`. Callers cannot branch on local-vs-wire (`port/src/lib.rs:24-27`). `prove_eq` is the in-process vs socket adapter contract (`54-57`).

**`lilo-sys`**: OS primitives (creds, process, signal, ipc). Domain mapping stays out of this crate.

---

## 5. Persistence

### 5.1 Schema (single migration)

`internal/db/migrations/0001_unified_schema.sql`.

| Table | Context | PK | Notes |
| --- | --- | --- | --- |
| `identity_audit` | Identity | `id TEXT` | `seq BIGINT GENERATED ALWAYS AS IDENTITY` for order; `timestamp` may collide. `owner` default `'local'`. `session_ref TEXT`. |
| `session_sessions` | Session | `id TEXT` | `owner`, `state TEXT`, `runtime_pid BIGINT NOT NULL`, `tmux_pane TEXT`. Index `(owner, namespace, terminated_at)`. |
| `session_namespaces` | Session | `slug TEXT` | Seeded `default`. |
| `messages` | Session (unprefixed) | `message_id TEXT` | `sender_ref TEXT`, `context_id TEXT`, optional idempotency unique. |
| `message_deliveries` | Session (unprefixed) | `(message_id, recipient_session_id)` | status CHECK `unread\|read\|undeliverable`. |
| `session_labels` | Session | `(session_id, key)` | |
| `session_event_cursor` | Session | `id = 1` | singleton BYTEA cursor. |
| `session_spawn_intents` | Session | `session_id` | status CHECK `pending\|resolved\|aborted`. JSON blobs. millis timestamps. |
| `runtime_lifecycle` | Runtime | `session_id TEXT` | `owner`, isolation, shim/runtime pids, tmux_pane TEXT, exit code/signal, lost_evidence. |
| `runtime_metadata` | Runtime | `key TEXT` | e.g. last probe sweep. |

Owner seam (`0001_unified_schema.sql:3-7`): `owner TEXT NOT NULL DEFAULT 'local'` on `session_sessions`, `runtime_lifecycle`, `identity_audit`. v1 writes `'local'`, no RLS. Future hosting can add `USING (owner = current_setting('lilo.owner'))` without backfill. **No code path currently filters by owner.**

**Filesystem (not Postgres):**

| Path | Owner | Content |
| --- | --- | --- |
| `$LILO_HOME/run/lilod.sock` | composed daemon | JSON-line `LilodRpc` |
| `$LILO_HOME/run/lilod.pid` | composed daemon | pidfile |
| `$LILO_HOME/data/events/runtime.jsonl` | Runtime daemon | `RuntimeEvent` stream |
| `$LILO_HOME/logs/` | both | `lilod.log`, per-session logs |
| `$LILO_HOME/settings.toml` | operator | `[database]` only |

### 5.2 Store discipline

- Session store uses `LiloDb` pool; writes `session_*` plus unprefixed mail tables.
- Runtime store uses the same pool; writes `runtime_*`.
- Identity store uses the same pool; writes `identity_audit`.
- **Cross-context write:** session spawn Tx A inserts `session_spawn_intents` **and** `runtime_lifecycle` Forking in one transaction (`handler/spawn.rs:96-136`). Session daemon holds a `LifecycleStore`. This is a real store-boundary leak: Session writes Runtime's table.
- Session event tail applies `RuntimeEvent` into `session_sessions` (`store/src/postgres/events.rs:23-50`). That is a **projection**, not a second source of truth for lifecycle. Source of truth for process state is `runtime_lifecycle` + JSONL; session state is the user-level projection.

### 5.3 Dual persistence of process facts

The same occupant is represented twice:

1. `runtime_lifecycle.state` = Forking/Running/Exited/Lost
2. `session_sessions.state` = SPAWNING/RUNNING/TERMINATED/LOST

Join is `SessionId`. There is no FK. Session completion builds `Session` from `SessionDraft` + `Lifecycle` (`spawn_intents.rs:95-127`). Drift is reconciled by the session runtime-event task and startup probe (`session.md:189-193`).

---

## 6. Lifecycle state machines

### 6.1 Session-backed spawn (current, implemented)

```text
SessionId::new()                              handler/spawn.rs:29
normalize_spawn_request (ns/dir)
build Session draft (state=Running, pid=0)    spawn.rs:44-63   // drafted as Running
PendingSpawnIntent { operation_id: IntentId::new() }
Tx A:
  Identity.authorize_in_tx(Action::Spawn)     spawn.rs:107-119
  insert session_spawn_intents pending
  insert runtime_lifecycle Forking            spawn.rs:125-131
runtime.spawn(id, launch)                     spawn.rs:72     // DIRECT to Runtime
  on err: abort intent
Tx B:
  complete intent (resolved)
  insert session_sessions Running
  update runtime_lifecycle Running
return RpcResponse::Spawned
```

Daemon invariant as documented: authorize, persist intent, drive runtime, persist evidence, respond (`session.md:137-138`). Recovery of pending intents is tested in `internal/session/daemon/tests/handler/spawn_recovery.rs`.

Draft is constructed as `SessionState::Running` before the process exists (`spawn.rs:52`). The row is not inserted until complete. `runtime_pid: 0` is a sentinel because the column is NOT NULL (`schema:40`).

### 6.2 Runtime process lifecycle

```text
Forking  --ShimReady--> Running --ShimExit/ProcessExit--> Exited
   |                      |
   +--------Lost----------+
```

Shim protocol: `ShimLaunch` (pull spec) -> child start -> `ShimReady` -> later `ShimExit`. Daemon treats ready/exit as lifecycle evidence (`runtime.md:54-58`). Reconciliation at startup and on probe sweep turns stale Running rows into current truth using process/shim/tmux/Docker evidence (`runtime.md:161-163`).

### 6.3 Spawn intent

```text
pending -> resolved (process running, session row written)
        -> aborted  (runtime spawn failed; reason stored)
```

### 6.4 Mail delivery

```text
insert messages + message_deliveries(unread)
read  -> read + read_at
recipient terminated before read -> undeliverable
```

### 6.5 Identity audit

Every authorize call writes an `AuditRow` (Allow or Deny) then returns. Audit is not optional. `identity_audit.seq` is the order key.

### 6.6 Target machines that do not exist

- Schedule: Never / OnFailure / Always restart (`schedule.md:111-116`).
- Transport: hold / interpret / overlay / forward (`transport.md:76-85`).
- Canvas interaction states (`canvas.md:62-71`).

---

## 7. Protocol / wire types

| Socket | Envelope | Payload |
| --- | --- | --- |
| Composed `lilod` (`~/.lilo/run/lilod.sock`) | `LilodRpc` tagged `{substrate, payload}` | `SessionRpc` or `RuntimeRpc` |
| Framing | JSON line | `read_optional_json_line` / `write_json_line` (`lilo-rm-core`) |
| Session responses | `RpcResponse` | Spawned, Listed, mail family, Capture, Doctor, Wait, Error, ... |
| Runtime responses | `RuntimeResponse` | Spawned, SpawnConflict, Status, Events, CursorExpired, ShimLaunch, Ack, Error, ... |
| Authn | SO_PEERCRED | `Principal::Local(uid)` before dispatch (`compose.rs:226-251`) |
| Diagnostic | `lilo runtime ...` | same `RuntimeRpc` via `lilo-rm-client` |

`lilo-wire` is a router enum only. It does not own framing, versioning, or auth.

MCP: session-core owns JSON-RPC envelope (`mcp.rs`) and authored tool contracts (`tool_contracts/`). Runtime has a parallel `tool_contracts.rs` and `mcp.rs` for the runtime MCP admin bridge. Two contract families.

---

## 8. Validation placement

Boundary discipline is mostly good: parse at the edge, trust internal types.

| Boundary | What is validated | Where |
| --- | --- | --- |
| CLI | clap args, runtime kind strings, mount specs, targets | `crates/lilo`, session-app, runtime-app; `MountSpec::from_str`, `SpawnTarget::from_str`, `TmuxAddress::from_str` |
| Selector | grammar, prefix charset/length | `selector/parser.rs`; store re-validates prefix (`sessions.rs:401-417`) |
| Namespace | slug, reserved `sm-`, not `default` on create | `Namespace::new` / `for_create` |
| Mail | intent (receipt reserved), notify/timeout pairing | `mail.rs:64-71,137-155` |
| SessionId | UUID parse | `FromStr` at store/driver edges (`conv.rs:171-175`) |
| Config | `settings.toml` deny unknown fields; missing file = default; present-but-bad = error | `settings.rs:18,51-57` |
| Env | owned `LILO_*` registry | `lilo-paths/src/env.rs`; `scripts/check-env.sh` |
| DB URL | refuse guessed host | `DbConfig::resolve` |
| Socket | peer uid | `peer_creds::extract` |
| Authz | local uid match | `AuditDecision::evaluate_local` |
| Runtime preflight | target occupancy, image, isolation | `RuntimeRpc::ValidateTarget`, daemon spawn |
| Runtime kind (session) | `claude`/`codex` only | `session/core/src/runtime.rs:33-38` |
| Runtime kind (runtime/identity) | `claude`/`codex` + `Other(String)` | `rm-core/types/runtime.rs:10-14`, `im-core/types.rs:164-168` |

Internal store codecs still parse strings (`state`, `runtime`, ids) because Postgres columns are TEXT. That is a second parse, not a second policy.

---

## 9. Dependency direction

### 9.1 Intended

```text
lilo / Canvas
    -> session-app -> session-daemon -> session-core
                                    -> session-store -> lilo-db
                                    -> identity-service -> im-core / im-store / im-stub
                                    -> session-driver -> rm-client -> runtime-daemon
                                                              -> rm-core
runtime-daemon -> runtime-store -> lilo-db
               -> launchers, lilo-sys, lilo-paths
```

Published crates must not depend on `internal/*`. `lilo-rm-core` is the exception that session-core **does** depend on.

### 9.2 Actual edges that matter

```text
lilo-session-core  -> lilo-rm-core     (IsolationPolicy, MountSpec, LostEvidence, NudgeMode)
lilo-session-store -> lilo-rm-core     (RuntimeSpawnRequest, Lifecycle, RuntimeEvent)
lilo-session-daemon-> lilo-runtime-store  (LifecycleStore insert_forking_in)
lilo-session-app   -> lilo-runtime-daemon (compose RuntimeService)
lilo-wire          -> session-core + rm-core
lilo-rm-client     -> lilo-wire
```

**Allowed (composition):** session-app composing both services into `lilod` (`compose.rs:206-252`). That is Phase 7, implemented.

**Problematic:**

1. Session-core depending on rm-core means the session **contract** imports runtime isolation/mount/lost types. A runtime protocol change ripples into the session wire.
2. Session daemon writing `runtime_lifecycle` couples spawn atomicity to a Runtime table. After Schedule exists, this shared tx cannot stay as-is.
3. Session store applying `RuntimeEvent` is a projection (acceptable) but imports the runtime event enum (tight coupling).

Runtime does **not** depend on session crates. Identity does **not** depend on session/runtime domain types beyond `SessionId` on `ResourceSpec`. That direction is clean.

### 9.3 Command surface vs context

`crates/lilo` is the unified verb surface (`cli.rs:234-313`): `run`, `create`, `get`, `delete`, `label`, `mail`, `nudge`, `capture`, `logs`, `wait`, `mcp` (session-backed) plus `runtime` (diagnostic) plus top-level `doctor` / `daemon`. No `lilo session ...` operator namespace in this CLI file (session verbs are top-level). No `lilo transport`. No `lilo identity`.

Raw `lilo runtime spawn` is identity-gated diagnostic access and creates no `session_record` or `session_spawn_intents` row (`system.md:83-85`).

---

## 10. Duplicate concepts across contexts

| Concept | Copies | Risk |
| --- | --- | --- |
| `SpawnRequest` | session-core `proto/spawn.rs:11` vs rm-core `types/spawn.rs:206` | Intentional DTO split. Driver `runtime_spawn_request` (`conv.rs:22-38`) is the translator. Session request has no id; runtime request has `SessionId`. |
| `RuntimeKind` | **three**: session-core Claude/Codex (`runtime.rs:10-13`); rm-core Claude/Codex/Other (`types/runtime.rs:10-14`); im-core Claude/Codex/Other (`types.rs:164-168`) | Session mapper drops `Other` (`conv.rs:189-194`). Identity's copy is unused by session spawn (session uses its own). Drift risk. |
| Capture | Runtime pane snapshot (`rm-core/capture.rs`) vs documented Transport capture lease (`transport.md:33-36`) vs session polish `lilo capture` | Same English word, three meanings. No Transport type exists. |
| Lifecycle vs SessionState | Forking/Running/Exited/Lost vs Spawning/Running/Terminated/Lost | Related but not isomorphic. Lost is shared type. Exited vs Terminated naming. |
| Tmux address | `TmuxAddress` (typed) vs `Session.tmux_pane: Option<String>` | Session loses structure. Schedule will need a third identity (pane id) (`schedule.md:52-64`). |
| `ChildExit.session_id` | `String` in driver (`conv.rs:103`) | Typed-id leak: converted to string at the port. |
| Tool contracts / MCP | `session-core/tool_contracts` and `rm-core/tool_contracts.rs` + `mcp.rs` | Two authored contract trees. |
| Prefix floors | `MIN_SHORT_PREFIX_LEN = 7` vs `MIN_SELECTOR_PREFIX_LEN = 4` | Human `short()` can be longer than the selector accepts as minimum; 4-char prefixes are more ambiguous on v4 ids. |
| Id encoding | sqlx `uuid` Type vs schema `TEXT` vs bind `to_string()` | Typed-id sqlx feature is decorative on real tables. |
| Mail table names | `messages` / `message_deliveries` vs `session_*` prefix on every other session table | Docs even mention this (`session.md:141-144`). Historic import residue. |
| IntentId in tests | `Uuid::now_v7()` fixtures vs production v4 | Allowed for snapshots (`Agents.md` typed-id note) but easy to copy into production. |
| `owner` column | present, always `'local'` | Prepared for multi-tenant RLS; unused. |

These are not all bugs. The two `SpawnRequest` types are a correct anti-corruption layer. Triple `RuntimeKind` and dual capture vocabulary are the expensive ones.

---

## 11. Data flow (current vs target)

### 11.1 Current `lilo run`

```text
operator
  -> crates/lilo Run(session_cli)
  -> Unix socket LilodRpc::Session(SessionRpc::Spawn)
  -> peer_creds -> Principal::Local(uid)
  -> IdentityClient.authorize_in_tx(Action::Spawn)
  -> session_spawn_intents pending + runtime_lifecycle Forking   [one TX]
  -> session-driver RuntimePort.spawn
       -> (in-process RuntimeService or rm-client)
       -> launchers + backend (host|docker) + shim
       -> ShimReady / lifecycle Running + JSONL event
  -> session_sessions Running + intent resolved
  -> RpcResponse::Spawned { Session }
```

No Transport. No Schedule. Capture lease is not attached.

### 11.2 Target (`system.md:70-81`)

```text
Session mints SessionId, persists occupant intent
Identity authorizes
Session asks Transport to prepare capture -> opaque lease
Session submits occupant + opaque payload to Schedule
Schedule places, binds, asks Runtime to execute
Runtime starts shim/harness without interpreting capture
Transport observes provider wire
Session exposes joined read model to Canvas
```

The current route "may implement this proof" if capture attachment stays opaque (`system.md:124-126`). **There is no type for that opaque payload today.** Session `SpawnRequest` and runtime `SpawnRequest` are fully interpreted.

### 11.3 Diagnostic spawn

`lilo runtime spawn` -> `LilodRpc::Runtime(RuntimeRpc::Spawn)` -> runtime only. No session row, no intent, no Schedule binding.

### 11.4 Reads

- `lilo get session` reads `session_sessions` (+ labels). Does not read `runtime_lifecycle` except via stored projection / event tail.
- `lilo runtime status` reads `runtime_lifecycle`.
- `lilo capture` is daemon-mediated polish: session resolves the session, then driver asks Runtime for a pane snapshot. Not a storage read of Transport.
- Canvas is specified to consume `lilod` read models only (`canvas.md:32-40`). No implementation.

---

## 12. Architectural strengths

1. **Context map is explicit and current.** `docs/architecture/{system,session,runtime,schedule,transport,canvas}.md` at this SHA name owners, non-owners, and deferrals. Code comments and crate names match the map more often than not.

2. **Typed ID family is DRY.** One macro, four types, v4 generation, transparent serde, tested short prefix. SessionId as join key is consistently used in domain structs.

3. **One Postgres, one migrator, one pool.** `LiloDb` + `0001_unified_schema.sql` + `EXPECTED_TABLES` test. Owner column reserved without fake multi-tenancy.

4. **Persist-intent-then-drive.** Spawn writes a pending intent and Forking lifecycle before calling Runtime, with abort on failure (`handler/spawn.rs:70-78`). Recovery tests exist.

5. **Anti-corruption driver.** `runtime_spawn_request` / `spawned_process` / nudge and capture mappers (`driver/src/conv.rs`) plus `PortError` opacity (`lilo-port`) keep session handlers off raw runtime faults.

6. **Mail vs nudge split.** Durable rows vs ephemeral delivery, shared routing, different persistence (`session.md:73-75`, mail tables vs `RuntimeRpc::Nudge`).

7. **Selector prefix with ambiguity.** Store-level `LIKE` + candidate list (`sessions.rs:163-182`) matches the typed-id note.

8. **Event cursor + status fallback.** Runtime does not pretend the JSONL is infinitely rewindable (`CursorExpired`, status reconcile).

9. **Identity at the library layer.** No fake identity CLI. Single `evaluate_local` rule shared by stub and in-tx path. Peer creds from `lilo-sys`, domain mapping in `lilo-im-core`.

10. **Published vs internal split.** `crates/` are versioned `0.8.0`; `internal/` is `publish = false`. `lilo-rm-core` is the smd↔rtmd compat contract (`version.rs`).

11. **Composed `lilod` exists.** `session-app/compose.rs` runs `SessionService` + `RuntimeService` on one socket with ordered shutdown (session tasks, runtime, socket, then DB). Integration tests: `session_spawn_contract.rs`, `shutdown_contract.rs`.

12. **Namespace and label grammar are owned by Session**, not leaked into Runtime tables.

---
