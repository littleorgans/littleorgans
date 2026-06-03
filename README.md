# littleorgans

Local-first control plane for running and observing coding agents on your own
machine. One operator, one host, one daemon. The command surface is `lilo`.

> **Status:** v0.8.0, pre-release with no external users. Breaking changes are
> expected as the design converges.

## What it is

`lilo` runs agent sessions (Claude, Codex) as managed processes on the local
host and gives you a kubectl-shaped surface to create, inspect, message, and
tear them down. A single daemon (`lilod`) owns the live state behind a unix
socket; the CLI is a thin client over it.

Four bounded contexts compose the daemon:

- **Session** is the control plane. It owns session records, intent
  reconciliation, mail, nudge, labels, and the user verbs that turn a request
  into running work.
- **Runtime** is the host executor. It launches processes, supervises the shim,
  adapts to platforms, and reports raw runtime status and lifecycle events.
- **Identity** is the local equivalent of a service account, RBAC, and audit. It
  authorizes at the library layer inside session and runtime; it has no command
  of its own yet.
- **Transport** is the wire-observation axis. It watches the bytes between an
  agent and its model provider and captures turns, independent of who spawned
  the process. It observes; it does not authorize, spawn, or reconcile.

The mental model is Kubernetes: session is the API server, runtime is the
kubelet, identity is the service account and RBAC, transport is mesh
observability, and `lilo` is `kubectl`.

## Install

```sh
just install          # build release and install the lilo binary
lilo doctor           # check local health
```

Or run from source without installing:

```sh
just lilo -- doctor
just lilo -- get session
```

## Command surface

User verbs are kubectl-shaped. Operator namespaces give explicit access to a
substrate. Run `lilo <command> --help` for flags and examples.

| Command | Purpose |
|---|---|
| `lilo run` | Run an agent session. |
| `lilo create session` | Create a session, label, or other resource. |
| `lilo get session` | Show sessions and other resources. |
| `lilo delete session` | Delete sessions and other resources. |
| `lilo label` | Update labels on a resource. |
| `lilo mail` | Send mail to an agent. |
| `lilo nudge` | Nudge an agent. |
| `lilo capture` | Capture session output. |
| `lilo logs` | Tail session logs. |
| `lilo wait` | Wait for a session condition. |
| `lilo mcp` | Run `lilo` as an MCP server. |
| `lilo runtime …` | Raw runtime operator namespace (diagnostic; never creates a session record). |
| `lilo session …` | Session substrate operator namespace. |
| `lilo doctor` | Inspect local `lilo` health. |

`lilo run` and `lilo create session` are session-backed. Raw `lilo runtime
spawn` is diagnostic access that stays identity-gated and appears only in
runtime status and events, never in `lilo get session`.

## Repository layout

This is the private monorepo; one version number covers the whole family.

| Path | Contents |
|---|---|
| `crates/` | Published crates (`lilo-` prefix), including the `lilo` binary. |
| `internal/` | Non-published substrate, grouped by context and role: `session/{app,core,daemon,driver,store}`, `runtime/{app,daemon,launchers,store}`, `identity/service`, plus shared `db`, `wire`, `port`. |
| `tools/` | Workspace tooling (`xtask`, future `mirror-publish`). |
| `docs/` | Architecture, reference, ADRs, mirror and provenance material. |
| `scripts/` | Repo gates (`check-env.sh`, `check-seam.sh`, `changed-crates.sh`). |

## Development

```sh
just check            # fmt, clippy, line caps, seam + env gates
just build            # build changed crates and their reverse-dep closure
just test             # nextest over the same scope
just regression       # unconditional full-workspace gate
```

Moon orchestrates the workspace for CI; Cargo remains the Rust source of truth,
so `cargo build --workspace` and `cargo test --workspace` work directly.

Run `just check && just build && just test` before every commit.

## Configuration

All local state lives under `~/.lilo/` (override the root with `LILO_HOME`):
config, run files, a single SQLite database at `data/lilo.db`, event JSONL,
logs, cache, and tmp. `lilo` owns exactly one environment prefix, `LILO_`,
sub-namespaced by audience. The full contract is
[`docs/reference/env-vars.md`](docs/reference/env-vars.md).

## License

MIT. See [LICENSE](LICENSE).
