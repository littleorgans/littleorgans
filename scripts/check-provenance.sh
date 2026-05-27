#!/usr/bin/env bash
set -euo pipefail

provenance_file="docs/provenance/imported-repos.md"
session_architecture_file="docs/architecture/session.md"
bad_runtime_map_label='untracked `runtime-matters/MAP.md`'
expected_runtime_note='Runtime architecture source artifacts at the frozen SHA included `runtime-matters/MAP.md` and `runtime-matters/PROJECT.md`. Phase 3/W4 merged them into `docs/architecture/runtime.md`.'
session_historic_crate_pattern='sm-(core|store|driver|daemon|cli|paths)'
session_allowed_provenance_pattern='provenance|historic|imported'

if grep -Fq "$bad_runtime_map_label" "$provenance_file"; then
    printf 'Provenance regression: %s must not label runtime-matters/MAP.md as untracked.\n' "$provenance_file" >&2
    exit 1
fi

if ! grep -Fxq "$expected_runtime_note" "$provenance_file"; then
    printf 'Provenance regression: %s must include the tracked runtime architecture source note.\n' "$provenance_file" >&2
    exit 1
fi

offending_session_lines="$(
    grep -En "$session_historic_crate_pattern" "$session_architecture_file" |
        grep -Evi "$session_allowed_provenance_pattern" || true
)"

if [[ -n "$offending_session_lines" ]]; then
    printf 'Provenance regression: %s has historic session crate names outside provenance/imported/historic lines:\n%s\n' \
        "$session_architecture_file" "$offending_session_lines" >&2
    exit 1
fi
