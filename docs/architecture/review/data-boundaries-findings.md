# Data boundaries findings and evidence

This file continues [Data boundaries](data-boundaries.md). It contains the detailed findings, explicit gaps, test evidence, file counts, and conclusion from the source review.

## 13. Real problems (with evidence)

1. **Session writes Runtime's table.** `DaemonState::begin_spawn_intent` calls `lifecycle_store.insert_forking_in` (`handler/spawn.rs:125-131`). Session daemon depends on `lilo-runtime-store`. Shared-tx atomicity is valuable today and becomes a Schedule blocker tomorrow.

2. **Session contract depends on Runtime contract.** `lilo-session-core/Cargo.toml:21` and `lib.rs:25` re-export `IsolationPolicy`/`MountSpec`. `LostEvidence` is a runtime type living on `SessionState`. Session wire compatibility is coupled to `lilo-rm-core`.

3. **Three `RuntimeKind` types.** Session rejects unknown runtimes; Runtime and Identity accept `Other`. Mapper is a closed match (`conv.rs:189-194`). Adding a runtime requires touching three enums plus the mapper.

4. **Capture name collision.** `lilo capture` is tmux pane snapshot (`rm-core/capture.rs`, session polish). Architecture docs use "capture" for Transport provider-wire leases. The first Transport slice will collide in CLI, RPC, and conversation unless renamed or namespaced (`lilo transport ...` is reserved but unused).

5. **No opaque launch payload type.** Target Schedule/Transport flow requires Session to attach an uninterpreted lease (`system.md:63-66`, `schedule.md:70-93`). Current spawn structs are fully concrete. The interim path can calcify.

6. **Docs vs identity enforcement.** `lilo-im-core` and `lilo-im-stub` crate docs say v1 does not enforce authorization. `evaluate_local` + stub + `authorize_in_tx` do enforce uid match. Reviewers will misread the contract.

7. **TEXT ids vs sqlx uuid Type.** Schema and codecs use strings. The `lilo-common/sqlx` feature is tested against a temp `uuid` column that production tables are not. Prefix `LIKE` works because of TEXT; a later uuid-column migration would break prefix search unless a text expression is kept.

8. **Selector prefix floor (4) vs short() floor (7).** Two constants, no shared source. 4-char prefixes on v4 ids raise ambiguity rates; `short()` will not emit 4-char ids.

9. **Draft Session constructed as Running with pid 0.** `spawn.rs:52-53`. The type cannot represent "not yet started" without lying (`runtime_pid` is `u32`, not `Option`). Schema `runtime_pid BIGINT NOT NULL` forces the lie. `SessionState::Spawning` is published (SQL, MCP schema, `session.md:151-154`) and has **no live writer**. Event SQL still filters `SPAWNING|RUNNING` (`events.rs:53-54, 81-82`). Docs that say "insert a spawning session row" are stale.

10. **Unprefixed mail tables.** `messages` / `message_deliveries` sit in the shared database without a `session_` prefix. Collision hazard if another context adds messaging.

11. **Wire crate is a stub.** Composition works, but there is no shared error envelope, version handshake, or request id. Session `RpcResponse::Error { message }` and runtime `ErrorPayload { code, message }` differ.

12. **Identity is not a service.** `IdentityClient` is an in-process stub. No `lilo-im-daemon`. RBAC/`Authorized.capabilities` are empty. Fine for v1 local-uid, but the crate graph pretends more than exists.

13. **Test fixtures still mint v7 `IntentId`s.** Production is v4. Not a runtime bug; it is a footgun next to the typed-id rule.

14. **Session-core `CaptureRequest` vs rm-core `CaptureRequest`.** Both exported names, different modules. Easy to import the wrong one.

15. **Identity `Action` vocabulary is overloaded.** `NamespaceCreate` is classified `Action::Kill` (`handler/authz.rs:31-33`). Session capture authorizes `Read` (`handler/sessions.rs:45`); the same pane snapshot on the runtime door authorizes `Logs` (`runtime/daemon/src/identity.rs:82-84`). Audit rows will not reconstruct the verb.

16. **`Authorized` is discarded.** `IdentityClient::authorize` returns `()` (`identity/service/src/client.rs:46-49`). `role: "admin"` and empty capabilities cannot affect callers. Deny is collapsed to `UnknownPrincipal` even for a known non-local uid.

17. **Door authz audits an empty resource.** List, wait, and namespace-get authorize `ResourceSpec::default()` (`authz.rs:22-27`). The audit log cannot say which sessions were listed. Namespace scope is applied only in the app CLI (`selector_scope.rs`); a raw `SessionRpc::List` over the socket is not scoped.

18. **Event tail does not mark mail undeliverable.** Incremental `RuntimeEvent::Terminated`/`Lost` only `UPDATE session_sessions` (`store/src/postgres/events.rs:64-96`). `mark_session_terminated` / `mark_session_lost` do call `mark_unread_undeliverable`. Lifecycle reap uses the latter. Event-driven terminate can leave unread counts lying.

19. **Two writers for the runtime event log.** Session-backed spawn pre-inserts Forking; Runtime sets `session_backed` and skips `insert_forking` (`runtime/daemon/src/server/spawn.rs:39-56`). `record_running(..., append_event: !session_backed)` (`api.rs:97-99`) so Session appends the Running event after its commit (`handler/spawn.rs` complete path). Diagnostic spawn appends in Runtime. One log, two authors.

20. **Runtime status swallows store errors.** `StatusReader::status` returns `Vec::new()` on list failure (`runtime/daemon/src/server/status.rs:21-29`). Clients can treat a broken store as an empty world.

21. **ShimReady timeout leaves Forking.** Launch failure calls `cancel_spawn` (`api.rs:86-90`). A 10s ready timeout returns `?` without deleting the row (`api.rs:93-96`).

22. **Publish graph cannot ship as declared.** `lilo-rm-client` (`publish` default true) depends on unpublished `lilo-wire` (`crates/lilo-rm-client/Cargo.toml:20`). Unified `lilo` depends on internal session/runtime/db crates. `lilo-im-store` stays publishable by not depending on `lilo-db` and duplicating audit DDL in tests.

23. **LostEvidence codecs disagree.** Runtime store writes `"PidNotAlive"` (`lifecycle/codec.rs:171`). Session store writes `"pid_not_alive"` (`events.rs:149-152`). Same enum, two strings in two tables.

24. **Spawn intent rows accumulate.** Schema comment says transient rows are "deleted on resolve" (`0001_unified_schema.sql:113-114`). Code updates status to resolved/aborted (`spawn_intents.rs` status update). No GC.

---

## 14. Explicit gaps

| Gap | Status | Evidence |
| --- | --- | --- |
| Schedule crate / schema / CLI | reserved | `schedule.md:1-4`; no workspace member |
| Transport crate / capture lease / provider adapters | none | `transport.md:1-4`; no `internal/transport` |
| Canvas / Desktop | none | `canvas.md:1-8`; no `apps/` content |
| Opaque launch payload | none | spawn types are fully interpreted |
| Transcript service between Transport and Session | open | `transport.md:71-74` |
| Transport table ownership | unlocked | same |
| `whoami` / `can-i` / audit CLI | reserved, not built | CLAUDE.md only |
| Enforcing identity daemon | stub only | `lilo-im-stub`, `lib.rs` comment |
| EventId / NamespaceId / Schedule ids | correctly absent | no stored field yet |
| FK from session to runtime lifecycle | absent | join by SessionId only |
| Owner RLS / multi-operator | column only | schema comment; no filter |
| Unmanaged session adoption | deferred | `session.md:31-32` |
| v2 / multi-host / k8s | out of scope | `NOTES/v1-v2-strategy.md` linked, not implemented |

Architecture docs still describe Phase 3/4 as separate daemons (`runtime.md:112-115`, `session.md:124-126`) while `compose.rs` already runs both services in one process. The composition hook (`SessionService::build`, `RuntimeService::build`) is used. The docs are slightly behind the binary.

---

## 15. Test evidence (invariants actually proven)

| Invariant | Test |
| --- | --- |
| All four id types Display/FromStr/serde | `crates/lilo-common/src/id.rs` `all_id_types_roundtrip_*`, `serde_json_roundtrip_*` |
| `SessionId::new()` is v4 | `new_generates_uuid_v4` |
| short() 7-char floor | `short_uses_seven_char_floor_for_singleton_context` |
| sqlx uuid roundtrip (temp table, ignored without DB) | `sqlx_postgres_insert_select_roundtrip` |
| Unified schema created on open | `internal/db/src/lib.rs` `open_postgres_runs_migrations_and_creates_unified_schema` |
| Env beats settings.toml | `internal/db/src/config.rs` `resolve_prefers_env_over_settings` |
| Lifecycle Forking→Running→Exited / Lost | `crates/lilo-rm-core/src/types/lifecycle.rs` transition tests |
| Protocol 0.8 + nudge wait capability | `version.rs` protocol tests |
| Principal Local tag | `lilo-im-core/src/types.rs` `serializes_local_principal_with_stable_kind_tag` |
| evaluate_local allow/deny | `audit.rs` three unit tests |
| Stub vs in-tx authz stay aligned | `internal/identity/service/tests/factory.rs` |
| `identity_audit.id` is TEXT (not uuid) | `lilo-im-store/tests/audit.rs` `assert_primary_key_is_uuid_column` (name is historical; asserts `data_type == "text"`) |
| CLI hides identity namespace | `crates/lilo/src/cli.rs` `help_lists_public_commands_and_hides_runtime_shim` |
| Session `authz_plan` is exhaustive | `handler/authz.rs` compile-time match + tests |
| Driver spawn mappers preserve lifecycle | `session/driver/src/conv.rs` `spawn_mappers_preserve_lifecycle_for_both_adapters` |
| Nudge/capture mapping | same file, `nudge_result_maps_runtime_outcomes`, `capture_result_wraps_runtime_response` |
| Port opacity | `internal/port/src/lib.rs` `opaque_wire_preserves_source` |
| Path tree / socket override | `lilo-paths/src/lilo.rs` path tests |
| Session spawn via composed lilod | `tests/integration/tests/session_spawn_contract.rs` |
| Compose shutdown order (tasks before DB) | `tests/integration/tests/shutdown_contract.rs` |
| Prefix / session store | `internal/session/store/src/postgres/sessions_tests.rs` |
| Mail store | `.../mail_tests.rs` |
| Spawn recovery | `internal/session/daemon/tests/handler/spawn_recovery.rs` |

I did not re-run `just test`. Claims above are from reading the tests, not from a fresh execution at this SHA.

---

## 16. Crate / LOC snapshot (fmm, source only)

```text
internal/session/   121 files  15,654
internal/runtime/    74 files  10,453
internal/db/          3 files     677
internal/port/        1 file      103
internal/identity/    2 files      93
internal/wire/        1 file        8

crates/lilo-rm-core        3,698
crates/lilo                2,263
crates/lilo-sys            1,314
crates/lilo-paths            743
crates/lilo-common           599
crates/lilo-im-core          451
crates/lilo-rm-client        412
crates/lilo-im-store         288
crates/lilo-build-support    118
crates/lilo-im-stub           62
```

Identity is two orders of magnitude smaller than Session. That matches "library layer, not a CLI/service" and also explains why RBAC is a type with no policy.

---

## 17. Bottom line

The implemented system is a **three-context local control plane** (Identity library, Session API, Runtime kubelet) with a **composed daemon** and a **unified Postgres schema**. The documented five-context product is real as design: Schedule, Transport, and Canvas are specified and empty.

Data ownership is mostly coherent: Session owns user meaning, Runtime owns process evidence, Identity owns audit and the uid gate, `SessionId` joins them. The expensive seams are already visible in code:

- Session→Runtime shared transaction and crate dependency
- triple `RuntimeKind`
- "capture" meaning pane snapshot today and provider wire tomorrow
- TEXT ids beside an unused uuid sqlx Type
- no opaque payload type for the Schedule cutover

Those are the places a first Transport/Canvas slice, or a Schedule activation, will hurt if left implicit.
