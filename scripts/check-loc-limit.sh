#!/usr/bin/env bash
set -euo pipefail

limit=700
failed=0

while IFS= read -r -d '' file; do
    lines=$(wc -l <"$file" | tr -d ' ')

    if [[ "$lines" -gt "$limit" ]]; then
        printf 'LOC limit exceeded: %s has %s lines; limit is %s\n' "$file" "$lines" "$limit" >&2
        failed=1
    fi
done < <(
    find . \
        \( -path './.git' -o -path './.moon/cache' -o -path './.nancy' -o -path './target' \) -prune -o \
        -type f \
        \( -name '*.rs' -o -name '*.ts' -o -name '*.tsx' -o -name '*.js' -o -name '*.jsx' -o -name '*.py' \) \
        -print0
)

exit "$failed"
