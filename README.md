# littleorgans

## Environment

littleorgans owns the `LILO_` environment namespace. The authoritative owned
name set lives in `lilo_paths::env`, and the repository gate rejects owned
names that are not registered there.

Audience model:

| Audience | Variables |
|---|---|
| Operator | `LILO_HOME`, `LILO_SOCKET_PATH`, `LILO_LOG`, `LILO_LOG_FORMAT`, `LILO_DOCKER_IMAGE`, `LILO_DOCKER_ALLOW_ROOT_IMAGE_USER`, `LILO_DOCKER_ALLOW_ARM64_MANIFEST_ESCAPE`, `LILO_PROBE_SWEEP_INTERVAL_MS`, `LILO_RESUME_POLL_INTERVAL_MS`, `LILO_RESUME_GAP_THRESHOLD_MS`, `LILO_TMUX_SERVER_LABEL` |
| Agent | `LILO_AGENT_SESSION_ID`, `LILO_AGENT_RUNTIME`, `LILO_AGENT_ROLE`, `LILO_AGENT_WORKSPACE` |
| Build/release | `LILO_CLI_VERSION`, `LILO_GIT_SHA`, `LILO_VERSION_INCLUDE_GIT_SHA` |
| Secret passthrough | `LILO_GITHUB_PAT` |
| Internal test/dev | `LILO_TEST_*`, `LILO_DEV_*` |

`LILO_LOG_FORMAT` accepts `auto`, `pretty`, `json`, and `compact`.
`LILO_SOCKET_PATH` overrides only the daemon socket. `LILO_DB_PATH` is not a
supported variable.

See `docs/reference/env-vars.md` for the full contract.
