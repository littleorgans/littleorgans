#!/usr/bin/env bash
set -euo pipefail

repo="$(git rev-parse --show-toplevel)"
cd "$repo"

printf 'head\t'
git rev-parse HEAD
printf 'status\t'
git status --short --branch

printf 'source_files_over_700\n'
find crates internal tools -type f -name '*.rs' -print0 |
    xargs -0 wc -l |
    awk '$1 > 700 && $2 != "total" {print $1 "\t" $2}'

metadata="$(cargo metadata --no-deps --format-version 1)"
printf 'publishable_depends_on_private\n'
jq -r '
  .packages as $packages
  | $packages[] as $package
  | select(($package.publish == null) or (($package.publish | length) > 0))
  | $package.dependencies[]
  | select(.kind == null and .path != null) as $dependency
  | ($packages[] | select(.name == $dependency.name)) as $target
  | select($target.publish == [])
  | [$package.name, $dependency.name, ($dependency.req // ""), $dependency.path]
  | @tsv
' <<<"$metadata" | sort

printf 'legacy_distributed_bins\n'
rg -n 'dist = true|name = "(sm|rtm)"' \
    internal/session/app/Cargo.toml \
    internal/runtime/app/Cargo.toml

printf 'active_strategy_storage_drift\n'
rg -n 'SQLite|SqlitePool|BEGIN IMMEDIATE' NOTES/v1-v2-strategy.md

printf 'untyped_session_runtime_boundary\n'
if rg -n \
    'session_id:[^,;)]*(str|String)|target:[^,;)]*(str|String)|signal:[^,;)]*(str|String)|parse_session_id|parse_runtime_signal' \
    internal/session/driver/src; then
    printf 'untyped Session to Runtime boundary found\n' >&2
    exit 1
fi

printf 'runtime_adapter_spawn_reconstruction\n'
if rg -n 'runtime_spawn_request\(' \
    internal/session/driver/src/in_process.rs \
    internal/session/driver/src/rtmd.rs; then
    printf 'Runtime adapter reconstructs a spawn request\n' >&2
    exit 1
fi

printf 'launch_attachment_child_delivery\n'
if rg -n 'launch_attachment|LaunchAttachment' \
    crates/lilo-rm-core/src/launcher.rs \
    crates/lilo-rm-core/src/types/lifecycle.rs \
    internal/runtime/app/src/cli/shim.rs \
    internal/runtime/daemon/src/docker_argv.rs \
    internal/runtime/daemon/src/docker_mount_plan.rs \
    internal/runtime/daemon/src/docker_runtime.rs; then
    printf 'launch attachment crossed the Runtime child delivery boundary\n' >&2
    exit 1
fi
if sed -n '1,/^#\[cfg(test)\]/p' \
    internal/runtime/daemon/src/shim_socket.rs | \
    rg -n 'launch_attachment|LaunchAttachment'; then
    printf 'launch attachment crossed the Runtime child delivery boundary\n' >&2
    exit 1
fi

printf 'alternate_session_socket_host\n'
if rg -n \
    'lilo_sys::ipc::bind|UnixListener::bind|run_daemon_with_db|pub async fn run_daemon|pub mod server;|pub use server::' \
    internal/session/daemon/src; then
    printf 'alternate Session socket host found under internal/session/daemon/src\n' >&2
    exit 1
fi

printf 'session_runtime_backdoors\n'
rg -n 'lifecycle_store|runtime_service' \
    internal/session/daemon/src/handler/state.rs \
    internal/session/daemon/src/handler/spawn.rs
