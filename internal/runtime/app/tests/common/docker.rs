use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use uuid::Uuid;

use super::RtmHarness;

pub fn write_fake_cli(dir: &Path) {
    let path = dir.join("docker");
    std::fs::write(&path, FAKE_DOCKER).expect("fake docker");
    let mut permissions = std::fs::metadata(&path).expect("metadata").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).expect("permissions");
}

pub fn container_pid(harness: &RtmHarness, session_id: Uuid) -> u32 {
    let path = harness
        .temp_path()
        .join("fake-docker-state")
        .join(format!("rtm-{session_id}.pid"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read fake container pid {}: {error}", path.display()))
        .trim()
        .parse()
        .expect("fake container pid")
}

pub fn container_env(harness: &RtmHarness, session_id: Uuid) -> Vec<String> {
    let path = harness
        .temp_path()
        .join("fake-docker-state")
        .join(format!("rtm-{session_id}.env"));
    std::fs::read_to_string(&path)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

pub fn container_image(harness: &RtmHarness, session_id: Uuid) -> String {
    let path = harness
        .temp_path()
        .join("fake-docker-state")
        .join(format!("rtm-{session_id}.image"));
    std::fs::read_to_string(&path).unwrap_or_default()
}

pub fn container_output(harness: &RtmHarness, session_id: Uuid) -> String {
    let path = harness
        .temp_path()
        .join("fake-docker-state")
        .join(format!("rtm-{session_id}.out"));
    std::fs::read_to_string(&path).unwrap_or_default()
}

const FAKE_DOCKER: &str = r#"#!/bin/sh
set -eu
state="$(dirname "$0")/fake-docker-state"
mkdir -p "$state"

stream_container_output() {
  name="$1"
  touch "$state/$name.out"
  tail -n +1 -f "$state/$name.out" &
  tail_pid="$!"
  pid="$(cat "$state/$name.pid")"
  while kill -0 "$pid" 2>/dev/null; do sleep 0.05; done
  kill "$tail_pid" 2>/dev/null || true
}

case "${1:-}" in
  --version)
    printf 'Docker version 27.0.0\n'
    ;;
  version)
    printf 'fake-docker\n'
    ;;
  image)
    shift
    [ "${1:-}" = "inspect" ] || exit 23
    shift
    shift
    format=""
    if [ "${1:-}" = "--format" ]; then format="$2"; fi
    case "$format" in
      "{{json .Config.User}}") printf '"1000"\n' ;;
      "{{json .Architecture}}") printf '"arm64"\n' ;;
      *) exit 23 ;;
    esac
    ;;
  manifest)
    printf '{"manifests":[{"platform":{"architecture":"arm64"}}]}\n'
    ;;
  run)
    shift
    name=""
    image=""
    env_values=""
    detached=0
    while [ "$#" -gt 0 ]; do
      case "$1" in
        --name) name="$2"; shift 2 ;;
        --env) env_values="${env_values}${2}
"; shift 2 ;;
        --label|--mount|--workdir) shift 2 ;;
        -d) detached=1; shift ;;
        --rm|-i|-t|--init|--sig-proxy=false) shift ;;
        --*) shift ;;
        *) image="$1"; shift; break ;;
      esac
    done
    command="$1"; shift
    case "$command" in
      */*) ;;
      *) [ ! -x "$(dirname "$0")/$command" ] || command="$(dirname "$0")/$command" ;;
    esac
    while IFS= read -r entry; do
      [ -n "$entry" ] || continue
      export "$entry"
    done <<EOF
$env_values
EOF
    printf '%s\n' "$image" > "$state/$name.image"
    nohup "$command" "$@" > "$state/$name.out" 2>&1 < /dev/null &
    printf '%s\n' "$!" > "$state/$name.pid"
    printf '%s' "$env_values" > "$state/$name.env"
    if [ "$detached" = 1 ]; then
      printf '%s\n' "$name"
    else
      stream_container_output "$name"
    fi
    ;;
  inspect)
    shift
    format=""
    if [ "${1:-}" = "--format" ]; then format="$2"; shift 2; fi
    name="$1"
    case "$format" in
      "{{.Config.Env}}")
        printf '['
        sep=""
        while IFS= read -r line; do
          [ -n "$line" ] || continue
          printf '%s%s' "$sep" "$line"
          sep=" "
        done < "$state/$name.env"
        printf ']\n'
        ;;
      *) exit 23 ;;
    esac
    ;;
  attach)
    shift
    while [ "$#" -gt 1 ]; do
      case "$1" in
        --detach-keys) [ "$2" = "" ] || exit 21; shift 2 ;;
        --sig-proxy=false) shift ;;
        *) exit 22 ;;
      esac
    done
    name="$1"
    stream_container_output "$name"
    ;;
  container)
    name="$3"
    if kill -0 "$(cat "$state/$name.pid")" 2>/dev/null; then
      printf 'true\n'
    else
      printf 'false\n'
    fi
    ;;
  kill)
    shift
    signal="15"
    if [ "${1:-}" = "--signal" ]; then signal="$2"; shift 2; fi
    name="$1"
    kill "-$signal" "$(cat "$state/$name.pid")" 2>/dev/null || exit 1
    ;;
  *)
    printf 'unexpected docker command: %s\n' "$*" >&2
    exit 64
    ;;
esac
"#;
