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
v0.8.0. Transport remains external and out of scope for this monorepo phase;
do not pull transport source, observability, or wire ownership into `lilo`
while implementing identity, runtime, or session work.

## K8s mental model post-monorepo

`lilo` is the kubectl-shaped command surface. `internal/session` is the API
server boundary. `internal/runtime` is the kubelet-shaped host executor.
Identity is the local equivalent of ServiceAccount, RBAC, and audit.

`transport-matters` is external observability and not part of the local
control plane. After Phase 7, `lilod` is the composed daemon process behind
the local socket, with composition rooted in the session app layer and runtime
remaining a substrate behind that boundary.

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
Operator namespaces are explicit substrate access: `lilo runtime ...`,
`lilo session ...`, and `lilo identity ...`.

Identity operator verbs are `lilo identity whoami` and `lilo identity audit`.
They are read-only, identity-gated, and routed through `lilod`.

`lilo run` and `lilo create session` are session-backed paths. Raw
`lilo runtime spawn` is diagnostic runtime access, remains identity-gated, and
does not create a `session_record` or a `session_spawn_intents` row. It appears
only in runtime status and events, never in `lilo get session`.

Daemon lifecycle commands are `lilo daemon start`, `lilo daemon stop`, and
`lilo daemon status`. Plain status is a pure query. Readiness is explicit:
`lilo daemon status --wait[=timeout]` blocks until `lilod` accepts socket
connections or the timeout expires.

`lilo doctor` stays top-level and aggregates substrate health. Do not add
per-substrate `doctor` commands unless a later locked decision changes that
surface.

`lilo __runtime-shim` is hidden runtime plumbing. It must remain outside public
help and docs except where the hidden contract is being tested.

## Data and environment

All local state lives under `~/.lilo/` unless `LILO_HOME` overrides the root.
The derived tree includes config, run files, one SQLite database at
`data/lilo.db`, event JSONL, logs, cache, and tmp directories.

`LILO_SOCKET_PATH` overrides only the daemon socket. `LILO_LOG` controls the
tracing filter. `LILO_DB_PATH` does not exist, and legacy `RTM_*`, `SM_*`, and
`AGM_*` variables are not honored.

No automatic migration is promised from old local roots. Release notes may
tell Stuart how to stop old daemons and start fresh, but code should not carry
legacy path fallbacks.

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
