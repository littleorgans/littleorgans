# v0.5 RuntimeResponse Fixture Capture

These fixtures are daemon-emitted JSON lines from `lilo-rm-core-v0.5.0`.
The release tag resolves to commit `782b3e5e19c5`:

```bash
git worktree add --detach /tmp/runtime-matters-v0.5-fixtures lilo-rm-core-v0.5.0
cd /tmp/runtime-matters-v0.5-fixtures
just build
```

The capture used an isolated runtime home and a fake headless shim. The fake
shim lets the real v0.5 daemon emit `shim_launch`, `ack`, and `spawned` without
starting Claude.

```bash
export RTM_HOME=/tmp/runtime-matters-v0.5-fixture-home
export RTM_DB_PATH="$RTM_HOME/db.sqlite"
export RTM_SOCKET_PATH="$RTM_HOME/rtmd.sock"
export RTM_SHIM_PATH=/tmp/runtime-matters-v0.5-fake-shim
export RTM_CAPTURE_OUT=/tmp/runtime-matters-v0.5-fixtures-out
rm -rf "$RTM_HOME" "$RTM_CAPTURE_OUT"
mkdir -p "$RTM_HOME" "$RTM_CAPTURE_OUT"
```

Seed the event log before daemon start. This is the only seeded runtime data;
all response fixtures still come from the v0.5 daemon socket.

```bash
printf '%s\n' '{"seq":8,"ts_ms":1700000000000,"kind":"lost","payload":{"session_id":"018f6e28-0000-7000-8000-000000000002","evidence":"pid_not_alive"}}' > "$RTM_HOME/events.jsonl"
```

Create the fake shim:

```bash
cat > "$RTM_SHIM_PATH" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
session_id=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --session-id) session_id="$2"; shift 2 ;;
    *) shift ;;
  esac
done
printf '{"type":"shim_launch","payload":{"request":{"session_id":"%s"}}}\n' "$session_id" \
  | nc -U "$RTM_SOCKET_PATH" \
  | tail -n 1 \
  | jq -cS . > "$RTM_CAPTURE_OUT/shim_launch.json"
printf '{"type":"shim_ready","payload":{"ready":{"session_id":"%s","shim_pid":1,"runtime_pid":1,"start_time":"2023-11-14T22:13:20Z"}}}\n' "$session_id" \
  | nc -U "$RTM_SOCKET_PATH" \
  | tail -n 1 \
  | jq -cS . > "$RTM_CAPTURE_OUT/ack.json"
sleep 1
SH
chmod +x "$RTM_SHIM_PATH"
```

Start the daemon and define the capture helper:

```bash
/tmp/runtime-matters-v0.5-fixtures/target/debug/rtm daemon start > "$RTM_HOME/daemon.out" 2> "$RTM_HOME/daemon.err" &
daemon_pid=$!

rpc() {
  fixture="$1"
  request="$2"
  printf '%s\n' "$request" \
    | nc -U "$RTM_SOCKET_PATH" \
    | tail -n 1 \
    | jq -cS . > "$RTM_CAPTURE_OUT/$fixture"
}

until [ -S "$RTM_SOCKET_PATH" ] && printf '%s\n' '{"type":"version"}' | nc -U "$RTM_SOCKET_PATH" > /tmp/rtm-v0.5-version.json 2>/dev/null; do
  sleep 0.05
done
jq -cS . /tmp/rtm-v0.5-version.json > "$RTM_CAPTURE_OUT/version.json"
```

Capture each response:

```bash
rpc cursor_expired.json '{"type":"events","payload":{"since":6}}'
rpc events.json '{"type":"events","payload":{"since":7}}'
rpc validate_target.json '{"type":"validate_target","payload":{"request":{"target":"tmux:not-a-pane"}}}'
rpc error.json '{"type":"spawn","payload":{"request":{"session_id":"018f6e28-0000-7000-8000-000000000099","runtime":"missing-runtime","env":[],"cwd":"/tmp/rtm","target":{"type":"headless","payload":{}}}}}'
rpc spawned.json '{"type":"spawn","payload":{"request":{"session_id":"018f6e28-0000-7000-8000-000000000001","runtime":"claude","env":[{"key":"RTM","value":"1"}],"cwd":"/tmp/rtm","target":{"type":"headless","payload":{}}}}}'
rpc status.json '{"type":"status","payload":{"request":{"session_id":"018f6e28-0000-7000-8000-000000000001","session_ids":[],"updated_since":null,"runtime":null,"state":null}}}'
rpc nudge.json '{"type":"nudge","payload":{"request":{"session_id":"018f6e28-0000-7000-8000-000000000001","content":"wake up"}}}'
rpc capture.json '{"type":"capture","payload":{"request":{"session_id":"018f6e28-0000-7000-8000-000000000001","scrollback_lines":500}}}'
rpc watchers.json '{"type":"watchers"}'
rpc doctor.json '{"type":"doctor"}'
rpc mcp_bridge.json '{"type":"mcp_bridge","payload":{"request":{"line":"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}"}}}'
sleep 60 &
sleeper_pid=$!
rpc kill_by_pid.json "{\"type\":\"kill_by_pid\",\"payload\":{\"request\":{\"pid\":$sleeper_pid,\"signal\":15,\"grace_secs\":2}}}"
wait "$sleeper_pid" 2>/dev/null || true
rpc stopping.json '{"type":"stop"}'
wait "$daemon_pid"
```

`shim_launch.json` and `ack.json` are captured by the fake shim during the
`spawned.json` request. `kill_by_pid.json`, `doctor.json`, and launcher command
paths contain values from the capture host and must be updated together with
`wire_compat.rs` if the fixture set is recaptured.
