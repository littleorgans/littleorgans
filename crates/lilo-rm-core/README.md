# lilo-rm-core

Core protocol types for the Runtime Matters daemon.

`lilo-rm-core` owns the public JSON line contract shared by `rtm` clients and rtmd:

- spawn, status, nudge, kill, doctor, MCP bridge, and version request types
- runtime lifecycle and event types
- transport helpers for newline delimited JSON

The current runtime protocol version is `0.4`. `VersionInfo` advertises this
protocol version and these stable capability names:

| Capability | Contract |
| --- | --- |
| `structured_protocol_errors` | Error responses expose stable machine readable codes. |
| `headless_stdio_log_paths` | Headless spawn responses include stdout and stderr log paths. |
| `status_session_set_filter` | Status requests accept a set of session ids. |
| `status_updated_since_filter` | Status requests accept an updated time lower bound. |
| `typed_nudge_outcomes` | Nudge responses expose typed delivery outcomes. |
| `validate_target_preflight` | ValidateTarget checks a target string without spawning. |
| `events_cursor` | Events support durable cursor replay. |
| `tmux_pane_snapshot` | Tmux targets support on demand pane snapshot capture. |

## Tmux Pane Snapshot Contract

`RuntimeRpc::Capture` returns a one shot `PaneSnapshot` for a tmux backed
target. It is a terminal pane snapshot, not process stdout or stderr. Content
may include prompts, user input, ANSI escapes, and redraw artifacts. The daemon
preserves ANSI bytes and does not interpret truncation.

The default `scrollback_lines` value is `1000`. Tmux caps useful history by the
target pane's `history-limit`. `pane_history_lines` is the raw tmux
`#{history_size}` value at capture time, which counts scrollback history rather
than the visible pane. History already dropped before capture cannot be
detected.

## Doctor JSON Contract

`RuntimeResponse::Doctor` returns `DoctorResponse`. In protocol version `0.4`,
session-matters may treat these JSON field names and value kinds as stable:

- `version.version`, `version.git_sha`, `version.protocol_version`, and `version.capabilities`
- `socket_path` and `uptime_secs`
- `sqlite.applied`, `sqlite.total`, `sqlite.applied_descriptions`, and `sqlite.pending_descriptions`
- `lifecycles.forking`, `lifecycles.running`, `lifecycles.exited`, and `lifecycles.lost`
- `watchers.process_exit_watchers` and `watchers.shim_sockets`
- `launchers[].runtime`, `launchers[].command`, and `launchers[].error`
- `tmux.available`, `tmux.version`, and `tmux.error`
- `log_availability[].session_id` and `log_availability[].log_availability`
- `last_probe_sweep`
- `recent_lost[].session_id`, `recent_lost[].evidence`, and `recent_lost[].occurred_at`

Diagnostic values such as versions, git shas, paths, uptime, process counts,
watcher counts, launcher commands, tmux details, and timestamps vary by build
and host. Use `version.protocol_version` plus `version.capabilities` for
contract negotiation.

The daemon, CLI, platform, launcher, and store crates remain private implementation details.
