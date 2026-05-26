---
name: rtm-admin
description: Admin MCP tools for runtime-matters rtmd.
---

## Admin MCP Tools

| Tool | Purpose |
| --- | --- |
| `rtm_kill_by_pid` | Admin escape hatch that signals a runtime process by pid, waits for the grace period, then sends SIGKILL if the process remains alive. |
| `rtm_status` | Return rtmd Lifecycle rows, optionally filtered by session id, session set, runtime, lifecycle state, and updated time. |
| `rtm_version` | Return the rtmd package version, build git sha, protocol version, and advertised capabilities. |
| `rtm_watchers` | Return rtmd operator visibility counters for process exit watchers, pending shim socket waiters, and Events long poll waiters. |
