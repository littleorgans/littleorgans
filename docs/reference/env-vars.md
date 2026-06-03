# Environment variables

The environment surface is user facing configuration, and therefore a **contract**. This
document is its single definitional reference: what `lilo` reads, what it injects, what it
ignores, and the naming rule that governs all of it. It describes the **steady state** and names
only target (`LILO_`) variables; the rename mapping from pre-monorepo and former-brand names is
recorded in the migration plan, not here, so this file carries no forbidden literal.

> **Status:** v0.8 pre-release. This describes the current `LILO_` contract. The const registry
> in `lilo-paths` is the owned name source of truth, and `scripts/check-env.sh --check` consumes
> that registry.

## Naming & ownership rule

`lilo` owns exactly **one** env var prefix, `LILO_`, sub namespaced by **audience** so the prefix
tells you who the variable is for.

| Sub-namespace | Audience | Stability |
|---|---|---|
| bare `LILO_*` | **Operator**: human set runtime config. The consumer contract. | documented, stable |
| `LILO_AGENT_*` | **Agent**: injected by `lilo` into every spawned agent process. | cross process contract |
| `LILO_*` (version/sha nouns) | **Build/release**: set by CI/`build.rs`, baked into the binary. | release surface |
| `LILO_TEST_*` / `LILO_DEV_*` | **Internal**: test harness, fault injection, dev recipes. | no stability promise |

So "bare `LILO_` = consumer facing" is true by construction. `LILO_` marks ownership; the
sub-prefix marks audience.

- **Foreign: read as-is.** Variables owned by the OS, toolchain, or a third-party runtime
  (`HOME`, `SHELL`, `CARGO_*`, `CLAUDE_*`, `ANTHROPIC_*`) keep their upstream names; we never
  prefix them.
- **Forbidden:** the pre-monorepo legacy prefixes and the former cross-brand agent namespace. The
  exact forbidden tokens are defined **once**, in the gate (`scripts/check-env.sh`), which excludes
  itself from its own scan. This document and all other repo files refer to them descriptively, so
  the gate's own pattern is the only place a forbidden literal exists.

Status legend: **live** = implemented today.

## Operator variables: bare `LILO_*`

Set by the human operating `lilo`. All resolve to a sensible default when unset.

| Variable | Values | Default | Read by | Purpose | Status |
|---|---|---|---|---|---|
| `LILO_HOME` | path | `$HOME/.lilo` | `lilo-paths` `LiloHome::from_env` | Roots the entire state tree. | live |
| `LILO_SOCKET_PATH` | path | `<home>/run/lilod.sock` | `lilo-paths` `socket_path()` | Overrides only the daemon socket. | live |
| `LILO_LOG` | tracing `EnvFilter` | `info` | `lilo-common/logging.rs:26` | Log filter (the `RUST_LOG` analogue). | live |
| `LILO_LOG_FORMAT` | `auto`\|`pretty`\|`json`\|`compact` | `auto` | `lilo-common/logging.rs` | Log rendering (see below). | live |
| `LILO_DOCKER_IMAGE` | image ref | unset | `docker_preflight.rs` | Default container image for `--isolation docker`. | live |
| `LILO_DOCKER_ALLOW_ROOT_IMAGE_USER` | `1`/`true`/`yes` | off | `docker_preflight.rs` | Permit a root-user image. | live |
| `LILO_DOCKER_ALLOW_ARM64_MANIFEST_ESCAPE` | `1`/`true`/`yes` | off | `docker_preflight.rs` | Permit the arm64 manifest workaround. | live |
| `LILO_PROBE_SWEEP_INTERVAL_MS` | u64 ms | code default | `reconcile.rs` | Liveness probe sweep interval. | live |
| `LILO_RESUME_POLL_INTERVAL_MS` | u64 ms | code default | `reconcile.rs` | Resume poll interval. | live |
| `LILO_RESUME_GAP_THRESHOLD_MS` | u64 ms | code default | `reconcile.rs` | Resume gap threshold. | live |
| `LILO_TMUX_SERVER_LABEL` | string | unset | `server/config.rs:92` | Optional tmux server label. | live |

### `LILO_LOG_FORMAT`

`LILO_LOG` (filter) and `LILO_LOG_FORMAT` (rendering) are orthogonal.
`LILO_LOG_FORMAT` is trimmed and case-insensitive. Unknown values fail with an
input-validation diagnostic. Precedence:

1. **`LILO_LOG_FORMAT`** = `pretty`/`json`/`compact`: explicit wins.
2. else **`--output json`** implies JSON logs (preserves today's coupling).
3. else **`auto`**: pretty on a tty, JSON when piped.

This decouples *log* rendering from `--output` (command-result rendering). The former JSON-toggle
boolean is removed: subsumed by `LILO_LOG_FORMAT=json`.

## Agent contract variables: `LILO_AGENT_*`

Injected by the runtime launcher into every spawned agent process. The agent and the human UI
correlate a captured session to the control-plane spawn id through `LILO_AGENT_SESSION_ID`.

| Variable | Set to | Injected at | Read by (in-repo) | Status |
|---|---|---|---|---|
| `LILO_AGENT_SESSION_ID` | spawn UUIDv7 | `launchers/lib.rs:100`, `spawn.rs:395` | `mcp/server.rs:19`, `cli/mail.rs:261` | live |
| `LILO_AGENT_RUNTIME` | runtime kind | `launchers/lib.rs:104` | none | live |
| `LILO_AGENT_ROLE` | session role | `spawn.rs:399` | none | live |
| `LILO_AGENT_WORKSPACE` | workspace path | `spawn.rs:403` | none | live |

**Caller-env stripping.** When capturing the caller's environment for a spawned runtime, all
`LILO_AGENT_*` must be stripped before child identity is re-injected, so a child never inherits its
parent's identity. The denylist is a single `starts_with("LILO_AGENT_")` rule.

## Build and release variables: `LILO_*`

Consumed by `build.rs` / `lilo-build-support`, baked in via `cargo:rustc-env`. `git_sha` is
diagnostic; the smd↔rtmd compat gate is `RUNTIME_PROTOCOL_VERSION` + capabilities, not the SHA.

| Variable | Default chain | Read by | Status |
|---|---|---|---|
| `LILO_CLI_VERSION` | package version | each app `build.rs` → `src/{main,lib}.rs` | live |
| `LILO_GIT_SHA` | `LILO_GIT_SHA` → `GITHUB_SHA` → `git rev-parse --short=7 HEAD` | `lilo-build-support` `explicit_git_sha` | live |
| `LILO_VERSION_INCLUDE_GIT_SHA` | off | `lilo-build-support` `include_git_sha` | live |

The product version is a **single workspace version** (`[workspace.package] version`, inherited by
all crates via `version.workspace = true`); `--version` is identical across binaries. Distinct from
`RUNTIME_PROTOCOL_VERSION`, a separate wire-compat axis.

## Secret: `LILO_GITHUB_PAT`

A GitHub Personal Access Token. Provided by the operator (environment) or CI (`littleorgans/littleorgans`
Actions secret `LILO_GITHUB_PAT`). Forwarded to spawned agents when present (generic caller-env
passthrough, like `ANTHROPIC_API_KEY`); never logged. Status: live.

## Test and dev variables: `LILO_TEST_*` / `LILO_DEV_*`

Read only by the test harness or local dev recipes. No stability promise, but still bound by the
`LILO_` rule.

| Variable | Read by | Purpose | Status |
|---|---|---|---|
| `LILO_DEV_BIN` | `justfile:3` | Locally built `lilo` for dev recipes. | live |
| `LILO_TEST_BENCH_BIN` | `tests/common/mod.rs:183` | Bench binary override. | live |
| `LILO_TEST_BENCH_SAMPLES` | `tests/common/mod.rs:49` | Bench sample count. | live |
| `LILO_TEST_BIN` | `tests/common/mod.rs:295` | Test binary override. | live |
| `LILO_TEST_FAULT_NAMESPACE_BINDING_CLEAR` | `cli/delete.rs:108` | Fault injection. | live |
| `LILO_TEST_PRINT_ENV` | `spawn_target.rs:170,286` → `harness.rs:352` | Fake-runtime print env. | live |

Other test sentinels use `LILO_TEST_*`. The print-cwd file marker is `.lilo-print-cwd`.
Test fixtures that need arbitrary user env keys use neutral example names, not `LILO_` names.

## Foreign variables (read-only)

We read these; we do not own or prefix them. The detector's classification sets and the
`spawn_context` caller-env denylist must reconcile to this list (Item 10/11 acceptance).

- **OS / shell:** `HOME`, `SHELL`, `PATH`, `LANG`, `LC_ALL`, `LOGNAME`, `TERM`, `COLORTERM`,
  `USER`, `TMUX`, `TMUX_PANE`.
- **Third-party runtime (passthrough/denylist):** `CLAUDECODE`, `CLAUDE_CODE`,
  `CLAUDE_CODE_OAUTH_TOKEN`, `CLAUDE_CONFIG_DIR`, `CLAUDE_CODE_*`, `CLAUDE_PLUGIN_*`,
  `ANTHROPIC_API_KEY`, `CODEX`.
- **Toolchain / CI:** `CARGO_*`, `OUT_DIR`, `GITHUB_SHA`.

## Enforcement

- **Registry.** Every owned `LILO_*` name is a `pub const` in `lilo-paths/src/env.rs`.
- **Gate.** `scripts/check-env.sh` consumes that registry and runs in `just check` and `moon ci`.
  Its legacy check is a **raw-token scan** for the forbidden prefixes (defined in the script) across
  **all** authored files, independent of the inventory site-regexes, so exhaustiveness does not
  depend on enumerating call shapes. The script **excludes itself** from that scan (it is the one
  authoritative home for the forbidden literals; cf. `scripts/check-seam.sh`). The inventory
  additionally traces const-indirected reads. The owned name set check rejects any owned-looking
  Rust string literal whose name is not in `lilo-paths/src/env.rs`.
- **Literal consolidation.** P5 centralizes the owned name set and key env-read sites. The broader
  sweep that forbids all raw registered `LILO_` literals in production Rust source is tracked as
  Item 16.
- **No composed `Config` type, no config-file layer** beyond per-agent TOML. The registry is the
  minimal cohesion fix for v1.

## Lifecycle & conventions

- **Adding a variable:** land it in the registry, this document, and (if operator-facing) the
  README in the same change. Not in the registry → not part of the contract.
- **Removing a variable:** delete it and its readers. Deletion is proven by the diff and a green
  suite. **Do not add a test that asserts the removed variable stays gone**. A perpetual deletion
  guard is negative-value coverage and keeps the dead name alive. Enforcement is the gate, never
  per-name guard tests.
- **No aliases, no legacy or former-brand revival, no compatibility vocabulary.** Pre-release;
  breaking changes are expected when they simplify the design.

## Migration

The full old→new rename mapping, the deletions, and the disposition of every pre-v0.8 variable are
recorded in the migration plan (`~/.mdx/projects/littleorgans-env-config-cohesion-plan.md`) and the
release notes. Deliberately **not** here, so this contract names no forbidden literal. In steady
state this document is the whole contract. The only **introduced** surface is `LILO_LOG_FORMAT`
(above) and the `lilo-paths` const **registry**.

## Non-goals (v0.8)

- No config-file layer beyond per-agent TOML.
- No composed `Settings`/`Config` aggregate type (would be v2 scope creep).
- No automatic migration from old roots; release notes tell operators to start fresh.
