#!/usr/bin/env python3
"""Env-var inventory + legacy gate for the LILO_ namespace.

littleorgans owns exactly one env-var prefix, LILO_, sub-namespaced by audience.
No legacy namespace: RTM_, SM_, AGM_, and the former HELIOY_ namespace are
forbidden, prefixed or bare.

Two independent passes:

  1. LEGACY GATE (--check): a RAW TOKEN scan for the forbidden prefixes and bare
     tokens across ALL authored files (not just .rs, not only env-var call
     shapes), so exhaustiveness never depends on enumerating call sites. This
     file is the single authoritative home for the forbidden literals, so it
     EXCLUDES ITSELF from the scan (cf. scripts/check-seam.sh). Exit 1 on any hit.

  2. INVENTORY (default): classify every LILO_/HELIOY_/foreign env-var literal by
     audience for review. Best-effort; the gate, not the inventory, is the guard.

Usage:
  scripts/check-env.sh            Inventory + a legacy summary.
  scripts/check-env.sh --check    Legacy gate: exit 1 if any forbidden token.

Run directly (it is a python3 script); do not invoke via `bash`.
"""
from __future__ import annotations

import re
import sys
from collections import defaultdict
from pathlib import Path

PRUNE = {"target", ".git", ".moon", ".nancy", "node_modules"}
# Files that legitimately NAME the forbidden tokens to document/enforce the rule:
# the gate itself, and the narrative convention/instruction/lesson docs. These are
# descriptions, not references, so they are exempt (cf. scripts/check-seam.sh's
# allowlist). Fixtures, snapshots, code, and config are NOT exempt.
EXCLUDE = {"scripts/check-env.sh", "CLAUDE.md", "AGENTS.md", "LESSONS.md"}
SKIP_SUFFIX = {".db", ".db-shm", ".db-wal", ".png", ".jpg", ".jpeg", ".gif", ".ico", ".lock"}

# --- forbidden tokens (the single literal definition lives only here) ---
LEGACY_PREFIXED = re.compile(r"(?<![A-Za-z0-9_])(?:RTM|SM|AGM|HELIOY)_[A-Za-z0-9_]*")
LEGACY_BARE = re.compile(r"(?<![A-Za-z0-9_])(?:RTM|SM|AGM)(?![A-Za-z0-9_])")

# --- inventory (secondary): env-var literals by ownership ---
NAME = r'"([A-Z][A-Z0-9_]{2,})"'
SITES = [
    re.compile(r"env::var(?:_os)?\s*\(\s*" + NAME),
    re.compile(r"env::(?:set|remove)_var\s*\(\s*" + NAME),
    re.compile(r"\b(?:option_)?env!\s*\(\s*" + NAME),
    re.compile(r"\.env(?:_remove)?\s*\(\s*" + NAME),
    re.compile(r"LaunchEnv::new\s*\(\s*" + NAME),
    re.compile(r"emit_cli_version\s*\(\s*" + NAME),
    re.compile(r"emit_git_sha_env\s*\(\s*" + NAME),
    re.compile(r"duration_env\s*\(\s*" + NAME),
    re.compile(r"const\s+\w+\s*:\s*&(?:'static\s+)?str\s*=\s*" + NAME),
]
OWNED = "LILO_"
FOREIGN_PREFIX = ("CLAUDE", "ANTHROPIC_", "CARGO_", "GITHUB_")
OS_EXACT = {"HOME", "SHELL", "PATH", "LANG", "LC_ALL", "TERM", "COLORTERM",
            "USER", "LOGNAME", "TMUX", "TMUX_PANE", "OUT_DIR", "CODEX"}


def iter_files(repo: Path, exclude_self: bool):
    for path in sorted(repo.rglob("*")):
        if not path.is_file():
            continue
        rel = path.relative_to(repo)
        if PRUNE & set(rel.parts):
            continue
        if path.suffix in SKIP_SUFFIX:
            continue
        if exclude_self and str(rel) in EXCLUDE:
            continue
        yield rel, path


def read_lines(path: Path):
    try:
        return path.read_text(encoding="utf-8").splitlines()
    except (UnicodeDecodeError, OSError):
        return None


def legacy_hits(repo: Path):
    hits = []
    for rel, path in iter_files(repo, exclude_self=True):
        lines = read_lines(path)
        if lines is None:
            continue
        for n, line in enumerate(lines, 1):
            for m in LEGACY_PREFIXED.finditer(line):
                hits.append((m.group(0), str(rel), n))
            for m in LEGACY_BARE.finditer(line):
                hits.append((m.group(0), str(rel), n))
    return hits


def classify(var: str) -> str:
    if var.startswith(OWNED):
        return "owned"
    if var.startswith("HELIOY_"):
        return "forbidden (HELIOY_)"
    if any(var.startswith(p) for p in ("RTM_", "SM_", "AGM_")):
        return "forbidden (legacy)"
    if any(var.startswith(p) for p in FOREIGN_PREFIX) or var in OS_EXACT:
        return "foreign"
    return "other"


def inventory(repo: Path):
    groups = defaultdict(set)
    for rel, path in iter_files(repo, exclude_self=True):
        if path.suffix != ".rs":
            continue
        lines = read_lines(path)
        if lines is None:
            continue
        for n, line in enumerate(lines, 1):
            if line.lstrip().startswith("//"):
                continue
            for pat in SITES:
                for m in pat.finditer(line):
                    groups[classify(m.group(1))].add(m.group(1))
    return groups


def gate(repo: Path) -> int:
    hits = legacy_hits(repo)
    if not hits:
        print("env gate: clean — no RTM_/SM_/AGM_/HELIOY_ tokens (gate self-excluded).")
        return 0
    by_token = defaultdict(list)
    for token, rel, n in hits:
        by_token[token].append((rel, n))
    print(f"env gate FAILED — {len(hits)} forbidden-token references "
          f"({len(by_token)} distinct), outside the convention docs:", file=sys.stderr)
    for token in sorted(by_token):
        sites = by_token[token]
        print(f"\n  {token}  [{len(sites)}]", file=sys.stderr)
        for rel, n in sites:
            print(f"    {rel}:{n}", file=sys.stderr)
    print("\nRename to the LILO_ namespace (or move the literal into the gate).", file=sys.stderr)
    return 1


def report(repo: Path) -> None:
    hits = legacy_hits(repo)
    by_token = defaultdict(list)
    for token, rel, n in hits:
        by_token[token].append((rel, n))
    print(f"LEGACY (RTM_/SM_/AGM_/HELIOY_) — {len(hits)} refs, {len(by_token)} tokens "
          f"[gate exempts: {', '.join(sorted(EXCLUDE))}]")
    for token in sorted(by_token):
        print(f"  {token:<40} {len(by_token[token])} sites")

    print("\nINVENTORY by ownership (.rs env-var literals):")
    for key in ("owned", "forbidden (HELIOY_)", "forbidden (legacy)", "foreign", "other"):
        vars_ = inventory(repo).get(key)
        if not vars_:
            continue
        print(f"\n  {key}  [{len(vars_)}]")
        for v in sorted(vars_):
            print(f"    {v}")


def main(argv: list[str]) -> int:
    repo = Path(__file__).resolve().parent.parent
    if "--check" in argv:
        return gate(repo)
    report(repo)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
