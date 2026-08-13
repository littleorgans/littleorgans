# transport-matters integration

**Status:** design, planning. Open items flagged inline await a MoE panel pass
before they are treated as locked.
**Date raised:** 2026-06-05.

## What transport is today

`transport-matters` (currently `v0.2.18`, tagline "Context control plane for
coding agents") is a standalone Python tool, roughly 52k LOC of API plus a
React `www/` and an Electron `desktop/`. It is a launcher CLI: `transport-matters
claude [dir]` brings up a `mitmdump` proxy and spawns Claude Code together in one
session, routing traffic via `ANTHROPIC_BASE_URL` (Claude reverse proxy) or
`HTTPS_PROXY` plus a process-scoped CA cert (Codex explicit proxy). It records
every turn (raw request, parsed IR, curated request, audit metadata) to
`~/.transport-matters/workspaces/.../run_id/` plus a queryable SQLite index at
`~/.transport-matters/index.db`, and serves a FastAPI surface for inspection.

It has zero coupling to littleorgans today, but already carries the join hook:
`TRANSPORT_MATTERS_OWNED_NATIVE_SESSION_ID` plus `OWNED_SOURCE_DESCRIPTOR`. Stuart
has already landed many repo changes to ease the move.

## Scope of this integration

Migrate the **Python API only**. The `www/` React UI and Electron `desktop/` do
not move; the future littleorgans human UI is a separate consumer on its own
TS/Electron train. This integration is the capture engine, the storage, and the
operator surface.

## Decision 1: spawn-path inversion (settled)

Transport stops being an opt-in orthogonal observer and becomes the default launch
wrapper. The chain inverts:

```
before:  lilo run claude  ->  runtime execs  claude
after:   lilo run claude  ->  runtime execs  tm claude  ->  tm brings up the proxy + spawns claude
```

Capture is therefore a **side effect of every `lilo run`**, not a user action.
Nobody invokes capture; `tm` records the wire because it sits in the launch path.
`schedule-matters` will reuse the same `tm` entry when it lands, so `tm` is a
shared launch primitive that both runtime (for sessions) and schedule invoke.

Reconciliation: CLAUDE.md says transport "sits outside the control plane." That
still holds in the load-bearing sense (`tm` does not authorize, decide spawns, or
reconcile). It is no longer *orthogonal*, it is *interposed* in the launch path.
A one-line doc clarification, not a redesign.

## Decision 2: naming (settled by Stuart)

- CLI binary: `tm`.
- Crate / package: `transport`.
- `lilo capture` is **not** transport. It is runtime's tmux pane-capture verb,
  promoted to top-level `lilo capture` (see
  `lilo-operator-namespace-consistency.md`). CLAUDE.md currently misattributes it
  to transport; that is corrected as part of this work.

## Decision 3: command surface

Transport's user-facing surface is the operator namespace `lilo transport ...`,
parallel to `lilo runtime ...` and `lilo session ...`. The launchers collapse into
the runtime-invoked `tm` wrapper. Mapping from today's surface:

| today (`transport-matters`) | new home | caller |
|---|---|---|
| `claude` / `codex` | internal `tm claude` wrapper | runtime, during `lilo run` |
| `claude --no-claude` (proxy only) | `tm` proxy-only flag | diagnostic |
| `list` | `lilo transport list` | operator |
| `paths` | `lilo transport paths` | operator |
| `doctor` | folds into aggregate `lilo doctor` | (CLAUDE.md: no per-substrate doctor) |
| `version` | `lilo transport version` / aggregated | operator |
| inspect a session's wire + fidelity diff | new `lilo transport show <session>` | operator, agents |

Namespace mechanism caveat: `lilo runtime`/`lilo session` are in-process re-exports
of Rust app crates. Transport is Python, so `lilo transport` cannot re-export
in-process. It is the first namespace backed by a different-language substrate, so
it is implemented either as (a) the Rust contract crate reading transport's store
directly, or (b) `lilo transport` shelling to `tm`. This is resolved by Decision 4.

## Decision 4: `tm` packaging (recommended, for panel)

The fork is who owns the launcher process and in what language. Recommendation is a
**split**, the synthesis of "a" (Python runner) and "b" (Rust crate):

- **Runner stays Python.** `tm` is the renamed Python console script. mitmproxy is
  Python-first; keeping the proxy and launcher in one process avoids a second
  interpreter boundary inside the hot path and avoids porting the launch
  orchestration (port allocation, CA injection, env setup, child supervision for
  both the Claude reverse-proxy and Codex explicit-proxy paths).
- **Contract is a Rust crate `transport`.** It owns the typed boundary that `lilo`,
  runtime, and future `schedule-matters` depend on: the env var names, the
  source-descriptor schema, the `~/.lilo/` storage layout, a helper that builds the
  wrapped launch command (`["tm", "claude", ...]` plus `LILO_AGENT_SESSION_ID`),
  and a typed read model over captured-session metadata so `lilo transport show`
  has one schema both sides share.

So the process is "a", the contract is "b". This keeps `lilo` Rust-first and typed
at the boundary while leaving the proxy where mitmproxy already lives.

**Open for panel:** pure-Python (no Rust crate, `lilo transport` shells to `tm` for
everything) versus the split above. The split costs one cross-language schema
contract that must be versioned across two trains; pure-Python costs `lilo` a typed
in-workspace dependency and pushes the read surface to subprocess parsing.

## Decision 5: SessionId join (settled)

`tm` reads `LILO_AGENT_SESSION_ID` (already in the `lilo_paths::env` registry as an
agent-injected variable) and stamps captured records with it, retiring
`TRANSPORT_MATTERS_OWNED_NATIVE_SESSION_ID`. The session record is the join key;
`lilo transport show <session>` resolves by that id. `SessionId` is the typed
UUIDv4 newtype (`NOTES/typed-ids-and-v4-prefix.md`).

## Decision 6: state path (proposed)

Captured state moves from `~/.transport-matters/` to `~/.lilo/capture/`, derived
from `LILO_HOME`. Transport's `index.db` stays a **separate** SQLite file under
`~/.lilo/capture/`, not folded into the control-plane `data/lilo.db`: the index is
Python-written and folding two languages into one sqlite file is a coupling trap.

## Decision 7: env contract (proposed)

- Boundary variables that cross the `lilo` <-> `tm` line are `LILO_`-prefixed and
  registered: `LILO_AGENT_SESSION_ID` (join, exists), and `tm` derives storage from
  `LILO_HOME` rather than its own `TRANSPORT_MATTERS_STORAGE_DIR`.
- Transport-internal proxy knobs (`PROXY_PORT`, `WEB_PORT`, `UPSTREAM_URL`) keep a
  separate `TM_`/`TRANSPORT_` prefix. "littleorgans owns exactly `LILO_`" governs
  the registered owned set and `check-env.sh`; it does not forbid a separate train
  from carrying its own internal prefix, and these knobs are not lilo's to own.

## Decision 8: read surface (open)

The two stated consumers are agents inspecting/sharing captured sessions and the
future human UI. With the UI not migrating, the v1 read surface is `lilo transport
show <session>` (CLI, for operators and agents). Open: whether the FastAPI server
is retained headless for the future UI and agent HTTP consumers, or dropped in v1
with the CLI/contract-crate read model as the only surface. An MCP surface for
agents is a candidate but not required for v1.

## Decision 9: reliability (open)

The proxy is now in the hot path, so a `tm` startup failure can break `lilo run`
itself. Needs an explicit ruling: does `lilo run --no-capture` exist as an escape
hatch, and does a proxy failure fail the run or fall back to a direct (uncaptured)
spawn? Leaning: `--no-capture` exists; default is fail-closed (a run that cannot
capture errors rather than silently dropping observation), revisited if it proves
too brittle.

## Decision 10: phase and location (proposed)

- Location: `python/transport/` (the reserved `python/` placeholder activates here).
- Phase: a new phase **after** Phase 9 (`v0.8.0` cutover). Phases 0-9 are all Rust
  substrate composition and transport is not a `v0.8.0` blocker. The inversion
  touches the runtime launch path, the unified command surface (Phase 6), and the
  `~/.lilo/` cutover (Phase 5), so it lands cleanly only once those settle.
- Pre-work in the transport repo (rename to `tm`/`transport`, read
  `LILO_AGENT_SESSION_ID`, derive storage from `LILO_HOME`) can proceed
  independently before the in-monorepo landing.
- Synthesis Risk 10 ("audit shared wire types with session-matters during Phase 4")
  is folded in: if wire types are shared, the Rust contract crate is where they live.

## Open questions for the panel

1. Decision 4: split versus pure-Python.
2. Decision 8: retain FastAPI headless, or CLI/contract-crate read model only in v1.
3. Decision 9: fail-closed versus fall-back-to-direct on proxy failure.
4. Whether `lilo transport` follows recommendation B from
   `lilo-operator-namespace-consistency.md` (diagnostic/substrate residue only) or
   carries a richer surface, given it is subprocess/cross-language not in-process.

## CLAUDE.md reconciliations required

- Bounded contexts: rename to `tm`/`transport`, state the spawn-path inversion,
  correct the `lilo capture` misattribution to `lilo transport`, narrow "not yet
  fixed" to the items still open here, point to this note.
- K8s mental model: note transport is now the default launch wrapper, still outside
  authorize/spawn/reconcile.
- Command surface: add `lilo transport ...` as the third operator namespace.
