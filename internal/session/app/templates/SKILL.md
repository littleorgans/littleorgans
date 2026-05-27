---
name: session-matters
description: Control local Helioy sessions through smd via MCP tools.
---

# session-matters

Use this skill when you need to run, list, inspect, or terminate local Helioy sessions.

## MCP Tools

| Tool | CLI | Purpose |
|------|-----|---------|
| `session_run` | `sm run` | Start a session through the session-matters daemon and rtmd. Supports claude and codex runtimes, headless or tmux targets, docker isolation, image selection, Docker bind mounts, a role, a directory, a namespace, labels, and filesystem agent config resolution. The tool returns the persisted session record. |
| `agent_run` | `sm run` | Deprecated compatibility alias for session_run. Start a session through the session-matters daemon and rtmd. Supports claude and codex runtimes, headless or tmux targets, docker isolation, image selection, Docker bind mounts, a role, a directory, a namespace, labels, and filesystem agent config resolution. The tool returns the persisted session record. |
| `session_list` | `sm get session` | List session records known to the session-matters daemon. Supports the shared selector grammar. |
| `agent_list` | `sm get session` | Deprecated compatibility alias for session_list. List session records known to the session-matters daemon. Supports the shared selector grammar. |
| `session_get` | `sm get session` | Get one session record by id. The tool returns an error envelope when the id is unknown. |
| `agent_get` | `sm get session` | Deprecated compatibility alias for session_get. Get one session record by id. The tool returns an error envelope when the id is unknown. |
| `namespace_list` | `sm get namespace` | List namespace records known to the session-matters daemon. |
| `namespace_get` | `sm get namespace` | Get one namespace record by slug. The tool returns an error envelope when the slug is unknown. |
| `session_capture` | `sm capture` | Capture tmux pane scrollback for one selected session. |
| `agent_capture` | `sm capture` | Deprecated compatibility alias for session_capture. Capture tmux pane scrollback for one selected session. |
| `session_delete` | `sm delete session` | Terminate daemon owned sessions selected by selector. Defaults to SIGTERM with a five second grace period. |
| `agent_delete` | `sm delete session` | Deprecated compatibility alias for session_delete. Terminate daemon owned sessions selected by selector. Defaults to SIGTERM with a five second grace period. |
| `session_label` | `sm label` | Add or remove one label on sessions selected by selector. Mutations use key=value to set and key- to remove. |
| `agent_label` | `sm label` | Deprecated compatibility alias for session_label. Add or remove one label on sessions selected by selector. Mutations use key=value to set and key- to remove. |
| `mail_send` | `sm mail send` | Send durable mail to sessions selected by selector. |
| `mail_read` | `sm mail read` | Read unread mail for sessions selected by selector. Reads mark messages read unless peek is true. |
| `mail_check` | `sm mail check` | Return the unread mail count for sessions selected by selector without draining mail. |
| `mail_stop_check` | `sm mail stop-check` | Return the unread mail count for stop-hook decisions without draining mail. |
| `nudge` | `sm nudge` | Send an ephemeral nudge to sessions selected by selector. Tmux-backed runtimes deliver through rtmd; headless or ended runtimes return typed failure messages. |
| `logs` | `sm logs` | Read the transcript linked to one selected session. |
| `wait` | `sm wait` | Wait until a selector satisfies running, terminated, or count=N. |
| `doctor` | `sm doctor` | Report session-matters daemon health, LOST sessions, and runtime-matters status. |

## Selector Grammar

Grammar:
  all
  <uuid>
  id:<uuid>
  role:<name>
  namespace:<slug>
  dir:<path>
  label:<key>=<value>
  label:<key> in (v1, v2)
Examples:
  all
  019e44f9-...
  role:engineer
  namespace:default
  dir:/tmp/project
  label:app=nginx
  "label:app in (web, api)"

## Examples

### `session_run`

```json
{
  "dir": "/Users/you/code/session-matters",
  "labels": [
    "area=auth",
    "pri=high"
  ],
  "namespace": "project-alpha",
  "role": "engineer",
  "runtime": "claude",
  "target": "headless"
}
```

### `session_list`

```json
{
  "selector": "namespace:project-alpha"
}
```

### `session_get`

```json
{
  "id": "019e32e3-0000-7000-8000-000000000000"
}
```

### `session_capture`

```json
{
  "id": "019e32e3-0000-7000-8000-000000000000",
  "scrollback_lines": 500
}
```

### `session_delete`

```json
{
  "grace_secs": 5,
  "selector": "id:019e32e3-0000-7000-8000-000000000000",
  "signal": "SIGTERM"
}
```


## Session Control Workflow

Start runtime-matters with `rtm daemon start` before `smd`; session-matters requires runtime-matters protocol 0.6 or newer.
Use `session_run` to run a local session through the session-matters daemon.
Use `session_list` to inspect live and terminated sessions.
Use `session_get` before acting on one session id.
Use `session_capture` to read tmux pane scrollback for a tmux backed session.
Use `session_delete` to terminate daemon owned sessions.
Use `session_label` to add or remove labels on selected sessions.
Use `logs` for daemon-spawned headless transcripts.
Use `wait` and `doctor` for lifecycle and runtime-matters diagnostics.
Use `mail_send`, `mail_check`, and `mail_read` for durable session mail.
Use `nudge` for the ephemeral notification surface.
