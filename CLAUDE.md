<!-- markdownlint-disable-next-line MD013 MD041 -->
follows global rules in `~/.claude/CLAUDE.md`, items below are monorepo-specific additions

## Project identity and status

littleorgans is the private monorepo for v1 local-first `lilo`: one
operator, one host, and one `lilod` process. The repository is pre-release
with no external users, so breaking changes are expected when they simplify
the design.

The governing decision record is
`~/.mdx/projects/littleorgans-monorepo-migration--synthesis.md`, locked
through rev08. The v1/v2 strategy remains linked here:
<!-- markdownlint-disable-next-line MD013 -->
`/Users/alphab/Dev/LLM/DEV/helioy/littleorgans/littleorgans/NOTES/v1-v2-strategy.md`.
Do not expand v2 scope in v1 implementation work.

Direction doc decision #12 in
`~/.mdx/projects/helioy-product-direction.md` keeps the broader project name
internal. User-visible names, package names, UI copy, public docs, and mirror
output use littleorgans or `lilo`. Related context may cite the internal
MoE-warroom-consensus cm lesson id
`019e5dbb-53e6-7ae3-b842-cfaba18fe690`.

## Migration drivers

Atomic releases are the first driver: one version number covers the whole
family, and `v0.8.0` is the first monorepo release. Tight cross-component
refactors are the second driver: a contract change can move through producer,
consumer, tests, and docs in one review.

Single CI is the third driver. Moon orchestrates the workspace while Cargo
remains the Rust source of truth. Open-source distribution is the fourth
driver: public mirrors are a generated distribution surface, while the private
monorepo stays the working source.

One brand surface is the fifth driver. The public organization, mirrors,
binary, domain, docs, and install story converge on littleorgans and `lilo`.
Internal project framing stays internal.

## Bounded contexts

Identity owns authorization, audit, service-account style identity, and RBAC
shape. Runtime owns process launch, shim behavior, platform adapters, lifecycle
events, and raw runtime status. Session owns user-level session records,
intent reconciliation, mail, nudge, delete, and the user verbs that compose
runtime work into a session.

Schedule is reserved only. It has no crate, daemon, or command namespace in
v0.8.0.

Transport is migrating into the monorepo as the wire-observation and capture
context. It owns the wire between an agent and its model provider, together
with the harness transcript. It proxies agent traffic, captures turns, and
surfaces the fidelity diff between what the harness believed it sent and what
actually reached the provider. Two consumers drive it: agents inspecting and
sharing captured sessions, and the littleorgans human UI. It stays an
independent axis, observing the wire regardless of which path spawned the
process, so it provides observability and sits outside the identity, session,
and runtime control flow. The user verb `lilo capture` already anchors its
surface. Its crate names, daemon composition, state path, and migration phase
are not yet fixed, so do not invent them ahead of its migration phase. Captured
sessions must correlate to the control-plane UUIDv7 spawn id, the platform join
key, so agents and the UI can share a session by that id rather than by a
provider-minted conversation id.

## K8s mental model post-monorepo

`lilo` is the kubectl-shaped command surface. `internal/session` is the API
server boundary. `internal/runtime` is the kubelet-shaped host executor.
Identity is the local equivalent of ServiceAccount, RBAC, and audit.

`transport-matters` is the wire-observation axis migrating into the monorepo.
It provides observability and sits outside the local control plane: it watches
the wire and does not authorize, spawn, or reconcile. After Phase 7, `lilod` is
the composed daemon process behind the local socket, with composition rooted in
the session app layer and runtime remaining a substrate behind that boundary.

This vocabulary is a design contract, not a topology claim. v1 is local-first;
v2 mapping is linked from the strategy note and stays out of v0.8.0 scope.

## Repository layout

The locked target layout comes from synthesis §5 §1 plus the rev02 session
amendment. `crates/` contains published crates only. `internal/` contains
non-published substrate code grouped by context and role. Session uses the
five-subdir shape `internal/session/{app,core,daemon,driver,store}`.

`tools/` contains workspace tooling such as `xtask` and future
`mirror-publish`. `docs/` contains architecture, reference, mirror, provenance,
and ADR material. `apps/`, `packages/`, `python/`, `helix/`, `products/`, and
`infrastructure/` are reserved placeholders until their phases activate them.

Do not add per-substrate instruction files in Phase 1. A substrate may receive
`internal/<substrate>/CLAUDE.md` only when the root file becomes insufficient
for later migration work.

## Command surface and substrate-boundary rule

User verbs are kubectl-shaped: `lilo run`, `lilo create session`,
`lilo get session`, `lilo delete session`, `lilo label`, `lilo mail`,
`lilo nudge`, `lilo capture`, `lilo logs`, `lilo wait`, and `lilo mcp`.
Operator namespaces are explicit substrate access: `lilo runtime ...` and
`lilo session ...`. Identity has no command namespace until it owns real verbs
(`whoami` / `can-i` / audit); its authorization runs at the library layer
inside session and runtime, not as a CLI command.

`lilo run` and `lilo create session` are session-backed paths. Raw
`lilo runtime spawn` is diagnostic runtime access, remains identity-gated, and
does not create a `session_record` or a `session_spawn_intents` row. It appears
only in runtime status and events, never in `lilo get session`.

`lilo doctor` stays top-level and aggregates substrate health. Do not add
per-substrate `doctor` commands unless a later locked decision changes that
surface.

## Data and environment

All local state lives under `~/.lilo/` unless `LILO_HOME` overrides the root.
The derived tree includes config, run files, one SQLite database at
`data/lilo.db`, event JSONL, logs, cache, and tmp directories.

littleorgans owns exactly one environment prefix: `LILO_`. The authoritative
owned name set is the `lilo_paths::env` const registry, and
`scripts/check-env.sh --check` rejects unregistered owned names.

The audience model is:

- Bare operator variables: `LILO_HOME`, `LILO_SOCKET_PATH`, `LILO_LOG`,
  `LILO_LOG_FORMAT`, `LILO_DOCKER_IMAGE`,
  `LILO_DOCKER_ALLOW_ROOT_IMAGE_USER`,
  `LILO_DOCKER_ALLOW_ARM64_MANIFEST_ESCAPE`,
  `LILO_PROBE_SWEEP_INTERVAL_MS`, `LILO_RESUME_POLL_INTERVAL_MS`,
  `LILO_RESUME_GAP_THRESHOLD_MS`, and `LILO_TMUX_SERVER_LABEL`.
- Agent-injected variables: `LILO_AGENT_SESSION_ID`, `LILO_AGENT_RUNTIME`,
  `LILO_AGENT_ROLE`, and `LILO_AGENT_WORKSPACE`.
- Build/release variables: `LILO_CLI_VERSION`, `LILO_GIT_SHA`, and
  `LILO_VERSION_INCLUDE_GIT_SHA`.
- Secret passthrough: `LILO_GITHUB_PAT`.
- Internal test/dev variables: `LILO_TEST_*` and `LILO_DEV_*`.

`LILO_SOCKET_PATH` overrides only the daemon socket. `LILO_LOG` controls the
tracing filter. `LILO_LOG_FORMAT` accepts `auto`, `pretty`, `json`, and
`compact`. `LILO_DB_PATH` does not exist, and legacy `RTM_*`, `SM_*`, and
`AGM_*` variables are not honored.

No automatic migration is promised from old local roots. Release notes may
tell Stuart how to stop old daemons and start fresh, but code should not carry
legacy path fallbacks.

## Identifier format (locked rev01: typed id family + v4)

Decision locked 2026-06-03. Full design:
`NOTES/typed-ids-and-v4-prefix.md`. Introduce a typed id family and move
generation from UUIDv7 to UUIDv4. Not yet executed; the staged PR sequence and
acceptance live in the note.

`lilo-common` gains a `define_id!` macro and one newtype per id concept
(`SessionId`, `MessageId`, `EventId`, `IntentId`, `NamespaceId`, `AuditId`),
replacing bare `uuid::Uuid` at domain signatures across the workspace. The macro
is the single source of truth for each id's behaviour, so the family stays DRY.
The runtime "spawn id" is a `SessionId`, not a separate type. `Uuid` stays the
inner 128-bit value; the wire and disk key stays 36 chars.

Generation moves to v4 inside the constructor (`SessionId::new()` calls
`Uuid::new_v4()`). The workspace `uuid` feature becomes additive (`v4` + `v7`),
not a `v7 → v4` flip, so test fixtures keep `now_v7()` and snapshot ordering
stays deterministic. v4's uniform entropy is what lets a git-style short prefix
discriminate; v7 front-loads a timestamp, so recent ids collide on their leading
hex exactly when a short prefix would be used.

Representation is held invariant so the ~131-file sweep is mechanical and
format-stable: `Display`/`FromStr` stay full 36-char, serde is
`#[serde(transparent)]`, sqlx delegates to `Uuid` behind a `lilo-common/sqlx`
feature. The short form is a separate `short()` accessor on human surfaces only
(`lilo get session`, `lilo mail peek`), git-style adaptive with a 7-hex floor.
Prefix *selection* extends the existing `internal/session/core` `Selector` with a
prefix variant resolved by a store `WHERE id LIKE ? || '%'` query that errors
with candidates on ambiguity.

Old v7 rows coexist with new v4 ids; no DB migration. On execution, also flip the
parent `littleorgans/CLAUDE.md` join-key line and the transport spawn-id note
above from UUIDv7 to v4, update the `assert_uuid_v7` audit test to v4, and
resolve the two ordering spots named in the note (`runtime store lifecycle.rs`
bare `ORDER BY session_id`; `session store mail.rs` `message_id` tiebreak).

## Engineering standards

DRY is mandatory. Search before adding helpers, constants, types, modules, or
files. If an existing shape is close, refactor it so both callers share one
path. Delete old paths during migrations unless Stuart explicitly approves a
staged transition.

The hard limits are 700 lines per file and about 150 lines per function. Files
already over the limit must be decomposed before new code is added. Use fmm as
structural context when changing exports, call graphs, workspace members, or
refactor boundaries.

fmm is local generated navigation state. Regenerate with
`fmm generate && fmm validate` after file moves, workspace manifest changes,
generated surface refreshes, or structural review. Preserve context through
handover files when coordination knowledge is not recoverable from git.

## Build, test, and generated surfaces

Use `just check && just build && just test` before every commit. The root
`justfile` is the required operator surface even when the underlying checks are
Cargo fmt, clippy, build, and test commands.

`moon ci` must orchestrate the same gate set for CI. `cargo build --workspace`
and `cargo test --workspace` remain the direct Rust acceptance commands for
Phase 1 and for diagnosing Moon behavior.

Generated surfaces must have one authored source of truth. `tools/xtask`
currently exposes placeholder commands for `codegen`, `dist-check`, and
`mirror-publish`; do not hand-edit generated help, schemas, snapshots, or
reference docs once a generator owns them.

## Release and mirrors

Release-plz manages per-package crate tags using package-version tag names.
The release workflow creates the top-level binary tag such as `v0.8.0` only
after crate publication succeeds.

Before crate publication, review `crates/lilo-rm-core/src/version.rs`: the
hand-maintained `RUNTIME_PROTOCOL_VERSION` and capabilities list are the
smd↔rtmd compat contract, and build.rs `git_sha` has no `.git` to read from a
published crates.io tarball.

`lilo-mirror-publish` is a future data-driven tool under
`tools/mirror-publish`. Its manifest defines one mirror per substrate with
paths, public crates, binary metadata, README source, changelog filter,
previous-history URL, and excludes.

Mirror pushes are deterministic and may force-push generated state. Apply mode
must refuse unless registry dependencies already exist at the release version,
the remote matches the manifest, and `previous_history_url` is present.

## Closeout checklist

Follow the phase sequence and exit criteria from synthesis §5 §8 and the
day-one mechanics from synthesis §5 §9 verbatim. For issue work, update Linear
first, then the external Nancy checklist, then commit, then write handover.

Do not mark work complete until it has been proven. The normal proof is
`just check && just build && just test`, plus any narrower acceptance commands
listed by the issue. If a generated navigation refresh is part of the change,
also run `fmm generate && fmm validate`.

Before closing a phase, verify the user-visible contract directly: command
output, JSON shape, symlink target, line cap, lint output, remote state, or CI
result as appropriate. A clean claim without the concrete proof is incomplete.
