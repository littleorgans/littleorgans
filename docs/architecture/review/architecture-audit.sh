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
rg -n 'session_id:.*str|pub session_id: String|pub target: String' \
    internal/session/driver/src/port.rs \
    internal/session/driver/src/driver.rs \
    internal/session/driver/src/in_process.rs \
    internal/session/driver/src/rtmd.rs

printf 'session_runtime_backdoors\n'
rg -n 'lifecycle_store|runtime_service' \
    internal/session/daemon/src/handler/state.rs \
    internal/session/daemon/src/handler/spawn.rs
