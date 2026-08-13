# lilo-arch comment review

Read-only. Scope limited to the listed files. Findings are comments that conceal a workaround, stale migration phase, unenforced invariant, duplicate topology, or boundary leak. Public rustdoc that only names a type or method is omitted.

## Findings

### 1. stale migration phase + unenforced invariant

`internal/db/src/lib.rs:24`

```rust
/// Target Postgres pool handle. Phase 2 end state for every store caller.
```

`LiloPool` is the live handle. "Phase 2 end state" and "Target" freeze a finished SQLite-to-Postgres cutover. No other store path remains in this file.

### 2. stale migration phase + unenforced invariant

`internal/db/migrations/0001_unified_schema.sql:3-7`

```sql
-- Owner seam (Phase 0 decision 10): session_sessions, runtime_lifecycle, and
-- identity_audit carry `owner TEXT NOT NULL DEFAULT 'local'`. v1 writes 'local'
-- everywhere and enables no row-level security. A future hosting tier can add a
-- per-owner RLS policy (e.g. USING (owner = current_setting('lilo.owner')))
-- without a data backfill; `owner` is already folded into the listing indexes.
```

Phase 0 is closed. The comment is the only statement of "v1 writes local" and "no RLS". Schema default is not a write invariant. No CHECK, no FORCE RLS, no trigger. Hosting-tier RLS is a wish.

### 3. unenforced invariant

`internal/db/migrations/0001_unified_schema.sql:113-114`

```sql
-- Spawn-intent timestamps are epoch-millis BIGINT (transient rows, deleted on
-- resolve, no cross-table time comparison); deliberately not timestamptz.
```

BIGINT is encoded. "deleted on resolve" and "no cross-table time comparison" are conventions. The comment is the only guard.

### 4. stale migration phase + duplicate topology + boundary leak

`internal/runtime/daemon/src/api.rs:21-39`

```rust
/// Curated in-process runtime domain API reviewed under R1.
///
/// This is the public surface for co-located callers that need runtime behavior
/// without going through the socket RPC adapter. The methods intentionally mirror
/// runtime-owned verbs and return public payload types from `lilo_rm_core` or
/// standard library containers:
/// ...
/// Session vocabulary (`reap_exited` / `terminate` / `watch_events` /
/// `terminate_all`) is NOT on `RuntimeService`; it lives on the WS2
/// `RuntimePort` and maps onto these verbs.
```

R1 and WS2 are finished wave names. The comment is the map of two verb vocabularies and a blessed skip around the socket. Session daemon holds both `RuntimePort` and `RuntimeService` (`internal/session/daemon/src/handler/state.rs:24-25`) and calls `runtime_service.append_event` after commit (`spawn.rs:205-208`).

### 5. boundary leak + unenforced invariant

`internal/session/daemon/src/handler/state.rs:19-21`

```rust
    // Runtime lifecycle store sharing the unified database. Built from the same
    // `LiloDb` as `store`; the spawn path runs its lifecycle writes inside the
    // shared `LiloTransaction`, so this instance's pool is only a handle.
```

Session daemon owns `LifecycleStore` and writes runtime rows (`spawn.rs:128-131`, `174-177`). "only a handle" is false in the type. The comment hides a store-boundary leak as shared-pool trivia.

### 6. duplicate topology (stale dual-tx names)

`internal/session/daemon/src/handler/spawn.rs:330-333`

```rust
    /// Begin the shared spawn transaction as a single pool-scoped
    /// [`LiloTransaction`]. The caller threads `&mut tx` to store methods and to
    /// `authorize_in_tx`, then commits with [`commit_or_rollback`]. One begin,
    /// one finish, both pool-scoped.
```

Call sites still label two sequential transactions:

- `spawn.rs:104` `"failed to acquire session spawn Tx A connection"`
- `spawn.rs:167` `"failed to acquire session spawn Tx B connection"`

The rustdoc sells one tx. The strings keep the old Tx A / Tx B topology.

### 7. workaround

`internal/session/daemon/src/handler/spawn.rs:116-118`

```rust
            // Commit the audit row authorize_in_tx wrote, then surface the error.
            commit_or_rollback(tx, Ok::<(), anyhow::Error>(())).await?;
            return Err(error);
```

Denial commits a fake `Ok(())` so the audit row survives. The comment is the policy. `authorize_in_tx` cannot persist audit without a successful commit, so deny has to lie to `commit_or_rollback`.

### 8. duplicate topology + boundary leak

`internal/session/driver/src/rtmd.rs:247-249`

```rust
    fn terminate_all(&self) {
        // Remote rtmd drains its own shims during its own shutdown.
    }
```

Same `RuntimePort::terminate_all` contract. In-process drains (`in_process.rs:179-181` calls `RuntimeService::drain_shims`). Remote is a no-op justified only by this line. The comment keeps a leftover standalone-rtmd topology next to composed `lilod`.

### 9. boundary leak

`internal/runtime/daemon/src/service.rs:106-107`

```rust
    /// Reap shims spawned by this service. Public so in-process owners (the
    /// session daemon, test harnesses) can drain without a full async shutdown.
```

Runtime-internal reaping is public because session and tests reach in. Pair with finding 8 and `DaemonState.runtime_service`.

### 10. workaround

`internal/runtime/daemon/src/service.rs:116-118`

```rust
        // Catch-all so shims never outlive an owner that dropped the service
        // without an explicit shutdown (e.g. a test harness with no teardown).
        self.state.drain_shims();
```

Drop-path drain as a safety net for missing teardown. The comment names the missing contract.

### 11. workaround

`internal/runtime/daemon/src/api.rs:184-185`

```rust
    // Outside pid_t range, so shutdown drain cannot signal an unrelated CI process.
    const TEST_SHIM_PID: u32 = u32::MAX;
```

Sentinel pid instead of a typed non-killable test handle. Drain behavior depends on this magic.

### 12. unenforced invariant

`crates/lilo/src/cli.rs:367-372`

```rust
    // Scope: clap-parse validity only, flag existence, required args, and
    // FromStr-flag syntax (the isolation enum, MountSpec). It does NOT validate
    // runtime semantics: selectors and `--target` are plain strings, and
    // `docker:PROFILE` accepts any name, so example semantics (real selectors,
    // valid tmux targets, real docker profiles) are gated by review, not by
    // this test.
```

Documented example semantics are admitted unenforced. Review is the gate.

### 13. stale topology / boundary leak

`crates/lilo-rm-client/src/lib.rs:3-7` and `:35`

```rust
//! Async Unix socket client for the public rtmd JSON line contract.
//!
//! `lilo-rm-client` owns connection setup, newline delimited JSON framing, and
//! typed client side error normalization. Protocol request and response shapes
//! remain in `lilo-rm-core`.
```

```rust
/// Async client for the rtmd Unix socket JSON line protocol.
```

`request_on_stream` writes `LilodRpc::Runtime(rpc)` (`lib.rs:306`). The crate talks to composed `lilod` and still documents a standalone rtmd socket.

## Empty in scope

No comments in:

- `internal/session/daemon/src/service.rs`
- `internal/session/driver/src/port.rs`
- `internal/session/driver/src/in_process.rs`
- `internal/session/driver/src/conv.rs`
- `internal/wire/src/lib.rs`

`internal/session/app/src/compose.rs` comments are ReadyCheck / `RunMode` rustdoc. They narrate the smoke path. They do not conceal one of the five faults.

## Counts

- findings: 13
- files with findings: 9
- scoped files with no comments: 5
- writes: this report only
