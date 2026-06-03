#!/usr/bin/env bash
set -euo pipefail

if ! command -v python3 >/dev/null 2>&1; then
    printf 'Seam lint requires python3 on PATH.\n' >&2
    exit 1
fi

files=()
while IFS= read -r -d '' file; do
    files+=("$file")
done < <(
    find . \
        \( -path './.git' -o -path './.moon/cache' -o -path './.nancy' -o -path './target' -o -path './crates/lilo-sys' \) -prune -o \
        -type f \
        -name '*.rs' \
        ! -path '*/tests/*' \
        ! -path '*/benches/*' \
        ! -path '*test_support*' \
        -print0
)

python3 - "${files[@]}" <<'PY'
from __future__ import annotations

import bisect
import re
import sys
from pathlib import Path

SIG_ALLOWLIST = Path("internal/runtime/daemon/src/signal.rs")

FORBIDDEN = [
    ("raw cfg", re.compile(r"\bcfg\s*\(\s*(unix|windows|target_os|target_family)\b")),
    ("UnixListener", re.compile(r"\bUnixListener\b")),
    ("UnixStream", re.compile(r"\bUnixStream\b")),
    ("std::os::unix", re.compile(r"\bstd::os::unix\b")),
    ("tokio::signal::unix", re.compile(r"\btokio::signal::unix\b")),
    ("SignalKind", re.compile(r"\bSignalKind\b")),
    ("libc::signal", re.compile(r"\blibc::signal\s*\(")),
    ("pre_exec", re.compile(r"\bpre_exec\b")),
    ("CommandExt", re.compile(r"\bCommandExt\b")),
    ("ExitStatusExt", re.compile(r"\bExitStatusExt\b")),
    ("getpeereid", re.compile(r"\bgetpeereid\b")),
    ("SO_PEERCRED", re.compile(r"\bSO_PEERCRED\b")),
    ("getuid", re.compile(r"\bgetuid\b")),
    ("pidfd", re.compile(r"\bpidfd\b")),
    ("kqueue", re.compile(r"\bkqueue\b")),
    ("libc::SIG", re.compile(r"\blibc::SIG[A-Z0-9_]+\b")),
]

# Masking only ever removes matches, so a file with no raw forbidden token
# cannot produce a post-mask hit. Prefilter on the union of the patterns and
# skip the O(chars) mask_rust() for the common clean file (~7.5s -> <1s).
ANY_FORBIDDEN = re.compile("|".join(f"(?:{pattern.pattern})" for _, pattern in FORBIDDEN))


def blank_preserving_newlines(text: str) -> str:
    return "".join("\n" if char == "\n" else " " for char in text)


def mask_rust(text: str) -> str:
    chars = list(text)
    i = 0
    block_depth = 0
    length = len(text)

    def blank(start: int, end: int) -> None:
        for offset in range(start, end):
            if chars[offset] != "\n":
                chars[offset] = " "

    while i < length:
        if block_depth:
            if text.startswith("/*", i):
                blank(i, i + 2)
                block_depth += 1
                i += 2
                continue
            if text.startswith("*/", i):
                blank(i, i + 2)
                block_depth -= 1
                i += 2
                continue
            if chars[i] != "\n":
                chars[i] = " "
            i += 1
            continue

        if text.startswith("//", i):
            end = text.find("\n", i)
            if end == -1:
                end = length
            blank(i, end)
            i = end
            continue

        if text.startswith("/*", i):
            blank(i, i + 2)
            block_depth = 1
            i += 2
            continue

        raw_start = raw_string_end(text, i)
        if raw_start is not None:
            end = raw_start
            blank(i, end)
            i = end
            continue

        if text[i] == '"' or text.startswith('b"', i):
            end = quoted_string_end(text, i + 1 if text[i] == '"' else i + 2)
            blank(i, end)
            i = end
            continue

        if text[i] == "'" and is_char_literal_start(text, i):
            end = char_literal_end(text, i + 1)
            blank(i, end)
            i = end
            continue

        i += 1

    return "".join(chars)


def raw_string_end(text: str, start: int) -> int | None:
    i = start
    if text.startswith("br", i):
        i += 2
    elif text.startswith("r", i):
        i += 1
    else:
        return None

    hashes = 0
    while i + hashes < len(text) and text[i + hashes] == "#":
        hashes += 1
    quote = i + hashes
    if quote >= len(text) or text[quote] != '"':
        return None

    terminator = '"' + ("#" * hashes)
    end = text.find(terminator, quote + 1)
    if end == -1:
        return len(text)
    return end + len(terminator)


def quoted_string_end(text: str, start: int) -> int:
    escaped = False
    i = start
    while i < len(text):
        char = text[i]
        if escaped:
            escaped = False
        elif char == "\\":
            escaped = True
        elif char == '"':
            return i + 1
        i += 1
    return len(text)


def is_char_literal_start(text: str, start: int) -> bool:
    next_index = start + 1
    return next_index < len(text) and not re.match(r"[A-Za-z_]", text[next_index])


def char_literal_end(text: str, start: int) -> int:
    escaped = False
    i = start
    while i < len(text):
        char = text[i]
        if escaped:
            escaped = False
        elif char == "\\":
            escaped = True
        elif char == "'":
            return i + 1
        elif char == "\n":
            return i
        i += 1
    return len(text)


def line_starts(text: str) -> list[int]:
    starts = [0]
    starts.extend(index + 1 for index, char in enumerate(text) if char == "\n")
    return starts


def line_for(starts: list[int], index: int) -> int:
    return bisect.bisect_right(starts, index)


def parse_attribute(masked: str, start: int) -> tuple[int, str] | None:
    i = start
    if i >= len(masked) or masked[i] != "#":
        return None
    i += 1
    while i < len(masked) and masked[i].isspace() and masked[i] != "\n":
        i += 1
    if i >= len(masked) or masked[i] != "[":
        return None

    depth = 1
    i += 1
    while i < len(masked):
        if masked[i] == "[":
            depth += 1
        elif masked[i] == "]":
            depth -= 1
            if depth == 0:
                return i + 1, masked[start : i + 1]
        i += 1
    return len(masked), masked[start:]


def compact_attr(attr: str) -> str:
    return re.sub(r"\s+", "", attr)


def is_test_gate_attr(attr: str) -> bool:
    compact = compact_attr(attr)
    if not compact.startswith("#[cfg(") or not compact.endswith(")]"):
        return False
    expression = compact[len("#[cfg(") : -2]
    if re.fullmatch(r"test", expression):
        return True
    return bool(re.search(r"\ball\([^)]*\btest\b", expression))


def is_test_cfg_attr(attr: str) -> bool:
    compact = compact_attr(attr)
    return compact.startswith("#[cfg_attr(test,")


def skip_ws(masked: str, index: int) -> int:
    while index < len(masked) and masked[index].isspace():
        index += 1
    return index


def next_attr_group(masked: str, start: int) -> tuple[int, int, list[str]] | None:
    index = skip_ws(masked, start)
    first = index
    attrs: list[str] = []

    while index < len(masked):
        parsed = parse_attribute(masked, index)
        if parsed is None:
            break
        end, attr = parsed
        attrs.append(attr)
        index = skip_ws(masked, end)

    if not attrs:
        return None
    return first, index, attrs


def item_end(masked: str, start: int) -> int:
    index = skip_ws(masked, start)
    paren_depth = 0
    bracket_depth = 0

    while index < len(masked):
        char = masked[index]
        if char == "(":
            paren_depth += 1
        elif char == ")" and paren_depth:
            paren_depth -= 1
        elif char == "[":
            bracket_depth += 1
        elif char == "]" and bracket_depth:
            bracket_depth -= 1
        elif char == ";" and paren_depth == 0 and bracket_depth == 0:
            return index + 1
        elif char == "{" and paren_depth == 0 and bracket_depth == 0:
            return block_end(masked, index)
        index += 1

    return index


def block_end(masked: str, start: int) -> int:
    depth = 0
    index = start
    while index < len(masked):
        char = masked[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                end = index + 1
                while end < len(masked) and masked[end].isspace() and masked[end] != "\n":
                    end += 1
                if end < len(masked) and masked[end] == ";":
                    end += 1
                return end
        index += 1
    return index


def test_ranges(masked: str) -> list[tuple[int, int]]:
    ranges: list[tuple[int, int]] = []
    index = 0

    while index < len(masked):
        group = next_attr_group(masked, index)
        if group is None:
            index += 1
            continue

        start, after_attrs, attrs = group
        if any(is_test_gate_attr(attr) for attr in attrs):
            end = item_end(masked, after_attrs)
            ranges.append((start, end))
            index = end
            continue

        attr_cursor = start
        for attr in attrs:
            parsed = parse_attribute(masked, attr_cursor)
            if parsed is None:
                break
            attr_end, _ = parsed
            if is_test_cfg_attr(attr):
                ranges.append((attr_cursor, attr_end))
            attr_cursor = skip_ws(masked, attr_end)

        index = after_attrs

    return ranges


def apply_ranges(masked: str, ranges: list[tuple[int, int]]) -> str:
    chars = list(masked)
    for start, end in ranges:
        for index in range(start, min(end, len(chars))):
            if chars[index] != "\n":
                chars[index] = " "
    return "".join(chars)


def scan_file(path: Path) -> list[str]:
    original = path.read_text(encoding="utf-8")
    if not ANY_FORBIDDEN.search(original):
        return []
    masked = mask_rust(original)
    sanitized = apply_ranges(masked, test_ranges(masked))
    starts = line_starts(sanitized)
    violations: list[str] = []

    for kind, pattern in FORBIDDEN:
        if kind == "libc::SIG" and path == SIG_ALLOWLIST:
            continue
        for match in pattern.finditer(sanitized):
            line_number = line_for(starts, match.start())
            line = original.splitlines()[line_number - 1].strip()
            violations.append(f"{path}:{line_number}: {kind}: {line}")

    return violations


def main() -> int:
    violations: list[str] = []
    for raw_path in sys.argv[1:]:
        path = Path(raw_path)
        display_path = Path(*path.parts[1:]) if path.parts and path.parts[0] == "." else path
        violations.extend(scan_file(display_path))

    if violations:
        print("Seam lint failed. Move OS seam code into crates/lilo-sys or an approved boundary:", file=sys.stderr)
        for violation in sorted(set(violations)):
            print(violation, file=sys.stderr)
        return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
PY
