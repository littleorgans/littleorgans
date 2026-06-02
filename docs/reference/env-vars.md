# Environment variables

The environment surface is user-facing configuration, and therefore a **contract**. This
document is its single definitional reference: what `lilo` reads, what it injects, what it
ignores, and the naming rule that governs all of it. It describes the **steady state** and names
only target (`LILO_`) variables; the rename mapping from pre-monorepo and former-brand names is
recorded in the migration plan, not here, so this file carries no forbidden literal.

> **Status:** v0.8 pre-release. This describes the **target contract**. Names not yet implemented
> and enforcement not yet built are both marked. The migration is tracked and executed by the
> env-namespace batch (`~/.mdx/projects/littleorgans-env-config-cohesion-plan.md`).
>
> **Enforcement is target, not current.** The const registry in `lilo-paths` (batch Item 11) and
> the hardened `scripts/check-env.sh` gate (Item 10) are the intended single source of truth and
> guard. Until they land, the gate is **partial** — see [Enforcement](#enforcement-target).

## Naming & ownership rule

`lilo` owns exactly **one** env-var prefix, `LILO_`, sub-namespaced by **audience** so the prefix
tells you who the variable is for.

| Sub-namespace | Audience | Stability |
|---|---|---|
| bare `LILO_*` | **Operator** — human-set runtime config. The consumer contract. | documented, stable |
| `LILO_AGENT_*` | **Agent** — injected by `lilo` into every spawned agent process. | cross-process contract |
| `LILO_*` (version/sha nouns) | **Build/release** — set by CI/`build.rs`, baked into the binary. | release surface |
| `LILO_TEST_*` / `LILO_DEV_*` | **Internal** — test harness, fault injection, dev recipes. | no stability promise |

So "bare `LILO_` = consumer-facing" is true by construction. `LILO_` marks ownership; the
sub-prefix marks audience.

- **Foreign — read as-is.** Variables owned by the OS, toolchain, or a third-party runtime
  (`HOME`, `SHELL`, `CARGO_*`, `CLAUDE_*`, `ANTHROPIC_*`) keep their upstream names; we never
  prefix them.
- **Forbidden:** the pre-monorepo legacy prefixes and the former cross-brand agent namespace. The
  exact forbidden tokens are defined **once**, in the gate (`scripts/check-env.sh`), which excludes
  itself from its own scan. This document and all other repo files refer to them descriptively, so
  the gate's own pattern is the only place a forbidden literal exists.

Status legend: **live** = implemented today · **rename** = live under an old name, renaming to the
listed name · **new** = specified here, not yet implemented · **remove** = being deleted.

## Operator variables — bare `LILO_*`

Set by the human operating `lilo`. All resolve to a sensible default when unset.

| Variable | Values | Default | Read by | Purpose | Status |
|---|---|---|---|---|---|
| `LILO_HOME` | path | `$HOME/.lilo` | `lilo-paths` `LiloHome::from_env` | Roots the entire state tree. | live |
| `LILO_SOCKET_PATH` | path | `<home>/run/lilod.sock` | `lilo-paths` `socket_path()` | Overrides only the daemon socket. | live |
| `LILO_LOG` | tracing `EnvFilter` | `info` | `lilo-common/logging.rs:26` | Log filter (the `RUST_LOG` analogue). | live |
| `LILO_LOG_FORMAT` | `auto`\|`pretty`\|`json`\|`compact` | `auto` | **target** resolver feeding `select_format` | Log rendering (see below). | new |
| `LILO_DOCKER_IMAGE` | image ref | unset | `docker_preflight.rs` | Default container image for `--isolation docker`. | rename |
| `LILO_DOCKER_ALLOW_ROOT_IMAGE_USER` | `1`/`true`/`yes` | off | `docker_preflight.rs` | Permit a root-user image. | rename |
| `LILO_DOCKER_ALLOW_ARM64_MANIFEST_ESCAPE` | `1`/`true`/`yes` | off | `docker_preflight.rs` | Permit the arm64 manifest workaround. | rename |
| `LILO_PROBE_SWEEP_INTERVAL_MS` | u64 ms | code default | `reconcile.rs:27` | Liveness probe sweep interval. | rename |
| `LILO_RESUME_POLL_INTERVAL_MS` | u64 ms | code default | `reconcile.rs:29` | Resume poll interval. | rename |
| `LILO_RESUME_GAP_THRESHOLD_MS` | u64 ms | code default | `reconcile.rs:33` | Resume gap threshold. | rename |
| `LILO_TMUX_SERVER_LABEL` | string | unset | `server/config.rs:92` | Optional tmux server label. | live |

### `LILO_LOG_FORMAT` (new feature)

`LILO_LOG` (filter) and `LILO_LOG_FORMAT` (rendering) are orthogonal. **Not implemented today:**
`select_format(output_json, stderr_is_terminal)` is a pure function over two booleans and reads no
environment (`logging.rs:34-56`); `LogFormat` has only `Json`/`Pretty` (`logging.rs:11-14`).
Shipping requires a resolver that reads `LILO_LOG_FORMAT` and feeds `select_format`, a new
`LogFormat::Compact`, and a `.compact()` subscriber arm (`logging.rs:58-71`). Target precedence:

1. **`LILO_LOG_FORMAT`** = `pretty`/`json`/`compact` — explicit wins.
2. else **`--output json`** implies JSON logs (preserves today's coupling).
3. else **`auto`**: pretty on a tty, JSON when piped (today's implicit behaviour — non-breaking).

This decouples *log* rendering from `--output` (command-result rendering). The former JSON-toggle
boolean is removed: subsumed by `LILO_LOG_FORMAT=json`.

## Agent-contract variables — `LILO_AGENT_*`

Injected by the runtime launcher into every spawned agent process. The agent and the human UI
correlate a captured session to the control-plane spawn id through `LILO_AGENT_SESSION_ID`.

| Variable | Set to | Injected at | Read by (in-repo) | Status |
|---|---|---|---|---|
| `LILO_AGENT_SESSION_ID` | spawn UUIDv7 | `launchers/lib.rs:100`, `spawn.rs:395` | `mcp/server.rs:19`, `cli/mail.rs:261` | rename |
| `LILO_AGENT_RUNTIME` | runtime kind | `launchers/lib.rs:104` | — | rename |
| `LILO_AGENT_ROLE` | session role | `spawn.rs:399` | — | rename |
| `LILO_AGENT_WORKSPACE` | workspace path | `spawn.rs:403` | — | rename |

**Caller-env stripping.** When capturing the caller's environment for a spawned runtime, all
`LILO_AGENT_*` must be stripped before child identity is re-injected, so a child never inherits its
parent's identity. The denylist is a single `starts_with("LILO_AGENT_")` rule — which also closes a
current leak (today's denylist strips only the session-id and runtime entries, not role/workspace,
so raw `lilo runtime spawn` leaks them; tracked as batch Item 13, folded into the prefix rename).

## Build / release variables — `LILO_*`

Consumed by `build.rs` / `lilo-build-support`, baked in via `cargo:rustc-env`. `git_sha` is
diagnostic; the smd↔rtmd compat gate is `RUNTIME_PROTOCOL_VERSION` + capabilities, not the SHA.

| Variable | Default chain | Read by | Status |
|---|---|---|---|
| `LILO_CLI_VERSION` | package version | each app `build.rs` → `src/{main,lib}.rs` | rename |
| `LILO_GIT_SHA` | `LILO_GIT_SHA` → `GITHUB_SHA` → `git rev-parse --short=7 HEAD` | `lilo-build-support` `explicit_git_sha` | live; lilo-rm-core adopts via new `emit_git_sha_env` |
| `LILO_VERSION_INCLUDE_GIT_SHA` | off | `lilo-build-support` `include_git_sha` | live |

The product version is a **single workspace version** (`[workspace.package] version`, inherited by
all crates via `version.workspace = true`); `--version` is identical across binaries. Distinct from
`RUNTIME_PROTOCOL_VERSION`, a separate wire-compat axis.

## Secret — `LILO_GITHUB_PAT`

A GitHub Personal Access Token. Provided by the operator (environment) or CI (`littleorgans/littleorgans`
Actions secret `LILO_GITHUB_PAT`). Forwarded to spawned agents when present (generic caller-env
passthrough, like `ANTHROPIC_API_KEY`); never logged. Status: rename.

## Test / dev variables — `LILO_TEST_*` / `LILO_DEV_*`

Read only by the test harness or local dev recipes. No stability promise, but still bound by the
`LILO_` rule.

| Variable | Read by | Purpose | Status |
|---|---|---|---|
| `LILO_DEV_BIN` | `justfile:3` | Locally built `lilo` for dev recipes. | rename |
| `LILO_TEST_BENCH_BIN` | `tests/common/mod.rs:183` | Bench binary override. | rename |
| `LILO_TEST_BENCH_SAMPLES` | `tests/common/mod.rs:49` | Bench sample count. | rename |
| `LILO_TEST_BIN` | `tests/common/mod.rs:295` | Test binary override. | live |
| `LILO_TEST_FAULT_NAMESPACE_BINDING_CLEAR` | `cli/delete.rs:108` | Fault-injection. | rename |
| `LILO_TEST_PRINT_ENV` | `spawn_target.rs:170,286` → `harness.rs:352` | Fake-runtime: print env. | rename |

All other test sentinels and the currently-unprefixed shell-resume sentinel (`shim.rs:316,328`)
move under `LILO_TEST_*`. The print-cwd file marker is renamed to `.lilo-print-cwd`. Test fixtures
that used a brand-named sample key in agent-config `[env]` tables (arbitrary user env, not a lilo
var) use a neutral `EXAMPLE_AGENT_NAME`.

## Foreign variables (read-only)

We read these; we do not own or prefix them. The detector's classification sets and the
`spawn_context` caller-env denylist must reconcile to this list (Item 10/11 acceptance).

- **OS / shell:** `HOME`, `SHELL`, `PATH`, `LANG`, `LC_ALL`, `LOGNAME`, `TERM`, `COLORTERM`,
  `USER`, `TMUX`, `TMUX_PANE`.
- **Third-party runtime (passthrough/denylist):** `CLAUDECODE`, `CLAUDE_CODE`,
  `CLAUDE_CODE_OAUTH_TOKEN`, `CLAUDE_CONFIG_DIR`, `CLAUDE_CODE_*`, `CLAUDE_PLUGIN_*`,
  `ANTHROPIC_API_KEY`, `CODEX`.
- **Toolchain / CI:** `CARGO_*`, `OUT_DIR`, `GITHUB_SHA`.

## Enforcement (target)

> Batch Items 10 and 11 implement these. The current `scripts/check-env.sh` is partial.

- **Registry.** Every owned `LILO_*` name is a `pub const` in one `lilo-paths` module. No other
  module re-declares an env-var literal. Today only `LILO_HOME`/`LILO_SOCKET_PATH` live there.
- **Gate.** `scripts/check-env.sh` consumes that registry and runs in `just check` and `moon ci`.
  Its legacy check is a **raw-token scan** for the forbidden prefixes (defined in the script) across
  **all** authored files — independent of the inventory site-regexes, so exhaustiveness does not
  depend on enumerating call shapes. The script **excludes itself** from that scan (it is the one
  authoritative home for the forbidden literals; cf. `scripts/check-seam.sh`). The inventory
  additionally traces const-indirected reads and masks inline `#[cfg(test)]`. Stays `.sh` +
  `python3` shebang (matches `scripts/changed-crates.sh`), invoked directly, never via `bash`.
- **Current-gate caveat (fix before trusting `--check`):** the site-regex + prefix-only
  classification lets live legacy through today — three reconcile-timing vars read via
  `duration_env`, a `.arg`-injected and an `OsString::from` test var, and bare-token forms — and
  mis-tags an inline-test var as `[PRODUCTION]`. Sequencing: the gate must forbid the former agent
  namespace only **with or after** that namespace's rename (Item 14), never before, or `just check`
  goes red mid-migration.
- **No composed `Config` type, no config-file layer** beyond per-agent TOML. The registry is the
  minimal cohesion fix for v1.

## Lifecycle & conventions

- **Adding a variable:** land it in the registry, this document, and (if operator-facing) the
  README in the same change. Not in the registry → not part of the contract.
- **Removing a variable:** delete it and its readers. Deletion is proven by the diff and a green
  suite. **Do not add a test that asserts the removed variable stays gone** — a perpetual deletion
  guard is negative-value coverage and keeps the dead name alive. Enforcement is the gate, never
  per-name guard tests.
- **No aliases, no legacy or former-brand revival, no compatibility vocabulary.** Pre-release;
  breaking changes are expected when they simplify the design.

## Migration

The full old→new rename mapping, the deletions, and the disposition of every pre-v0.8 variable are
recorded in the migration plan (`~/.mdx/projects/littleorgans-env-config-cohesion-plan.md`) and the
release notes — deliberately **not** here, so this contract names no forbidden literal. In steady
state this document is the whole contract. The only **introduced** surface is `LILO_LOG_FORMAT`
(above) and the `lilo-paths` const **registry**.

## Non-goals (v0.8)

- No config-file layer beyond per-agent TOML.
- No composed `Settings`/`Config` aggregate type (would be v2 scope creep).
- No automatic migration from old roots; release notes tell operators to start fresh.
