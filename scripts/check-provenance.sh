#!/usr/bin/env bash
set -euo pipefail

provenance_file="docs/provenance/imported-repos.md"
bad_runtime_map_label='untracked `runtime-matters/MAP.md`'
expected_runtime_note='Runtime architecture source artifacts at the frozen SHA included `runtime-matters/MAP.md` and `runtime-matters/PROJECT.md`. Phase 3/W4 merged them into `docs/architecture/runtime.md`.'

if grep -Fq "$bad_runtime_map_label" "$provenance_file"; then
    printf 'Provenance regression: %s must not label runtime-matters/MAP.md as untracked.\n' "$provenance_file" >&2
    exit 1
fi

if ! grep -Fxq "$expected_runtime_note" "$provenance_file"; then
    printf 'Provenance regression: %s must include the tracked runtime architecture source note.\n' "$provenance_file" >&2
    exit 1
fi
