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

Three bounded contexts compose the current daemon, with two designed for later
activation:

- **Session** is the control plane. It owns session records, intent
  reconciliation, mail, nudge, labels, and the user verbs that turn a request
  into running work.
- **Runtime** is the host executor. It launches processes, supervises the shim,
  adapts to platforms, and reports raw runtime status and lifecycle events.
- **Identity** is the local equivalent of a service account, RBAC, and audit. It
  authorizes at the library layer inside session and runtime; it has no command
  of its own yet.
- **Schedule** is reserved. When activated, it becomes the sole placement
  authority and reconciles desired topology and stable occupant bindings.
- **Transport** is planned as the provider wire observation axis. It captures
  exact traffic, interprets payloads, applies authorized transformations, and
  records fidelity evidence. It does not authorize, place, launch, or
  reconcile.

The mental model is Kubernetes: Session is the API server, Schedule is the
scheduler, Runtime is the kubelet, Identity is the service account and RBAC,
Transport is wire observation, and `lilo` is `kubectl`.

Canvas is the planned human workspace and Desktop is its native host. They are
one product surface. See the
[`system architecture`](docs/architecture/system.md) for current and target
flows.

## Install

```sh
just install          # build release and install the lilo binary
lilo doctor           # check local health
```

Or run from source without installing:

```sh
just lilo doctor
just lilo get session
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
| `lilo daemon` | Manage the local `lilo` daemon: `start` (foreground; `--ready-check` brings it up and exits), `stop`, `status`. |

`lilo run` and `lilo create session` are session-backed. Raw `lilo runtime
spawn` is diagnostic access that stays identity-gated and appears only in
runtime status and events, never in `lilo get session`.

## Repository layout

This is the private monorepo; one version number covers the whole family.

| Path | Contents |
|---|---|
| `crates/` | Published crates (`lilo-` prefix), including the `lilo` binary. |
| `internal/` | Non-published substrate, grouped by context and role: `session/{app,core,daemon,driver,store}`, `runtime/{app,daemon,launchers,store}`, `identity/service`, plus shared `db`, `wire`, `port`. Schedule activates here only after its boundary is proven. |
| `apps/`, `packages/`, `python/` | Reserved product and language workspaces. Their exact activation follows the Transport and Canvas architecture proof. |
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

Run `just check && just build && just test` before every commit. The
Postgres-backed `lilo-db` tests are `#[ignore]`d (the default suite skips
them); run them with `just test-db` after setting `LILO_TEST_DATABASE_URL`
or copying `settings.example.toml`. CI runs them automatically.

## Configuration

All local state lives under `~/.lilo/` (override the root with `LILO_HOME`):
config, run files, event JSONL, logs, cache, and tmp. The database is Postgres,
configured by `LILO_DATABASE_URL` (`LILO_HOME` no longer implies a database
path). `lilo` owns exactly one environment prefix, `LILO_`, sub-namespaced by
audience.

See [`docs/reference/postgres.md`](docs/reference/postgres.md) for database
setup across local native, Docker Compose, and cloud-managed Postgres, plus the
`lilo doctor` / `lilo daemon start --ready-check` connection smoke. The full
environment contract is [`docs/reference/env-vars.md`](docs/reference/env-vars.md).

## License

MIT. See [LICENSE](LICENSE).
