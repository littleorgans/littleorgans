#!/usr/bin/env python3
"""Emit -p flags for workspace crates whose source changed since base_ref,
plus the transitive reverse-dep closure. See gate-incremental in the justfile.

Usage:  scripts/changed-crates.sh [base_ref]
Output:
  - Empty line: no relevant changes; caller should skip the gate.
  - `--workspace`: change touched a workspace-wide file (root Cargo.toml,
                   rust-toolchain.toml, .cargo/*). Caller falls back to full gate.
  - `-p crateA -p crateB ...`: scope the gate to these crates.
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

WORKSPACE_WIDE_FILES = {"Cargo.toml", "rust-toolchain.toml"}
WORKSPACE_WIDE_PREFIXES = (".cargo/",)


def git(*args: str) -> list[str]:
    out = subprocess.run(["git", *args], capture_output=True, text=True)
    return [line for line in out.stdout.splitlines() if line]


def changed_files(base_ref: str) -> list[str]:
    if subprocess.run(
        ["git", "rev-parse", "--verify", base_ref], capture_output=True
    ).returncode != 0:
        base_ref = "HEAD~1"
    files: set[str] = set()
    files.update(git("diff", "--name-only", f"{base_ref}...HEAD"))
    files.update(git("diff", "--name-only", "HEAD"))
    files.update(git("diff", "--name-only", "--cached"))
    files.update(git("ls-files", "--others", "--exclude-standard"))
    return sorted(files)


def main() -> int:
    base_ref = sys.argv[1] if len(sys.argv) > 1 else os.environ.get("BASE_REF", "main")
    files = changed_files(base_ref)
    if not files:
        print("")
        return 0

    meta = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--format-version=1"],
            check=True, capture_output=True, text=True,
        ).stdout
    )
    workspace_ids: set[str] = set(meta["workspace_members"])
    pkg_name: dict[str, str] = {}
    pkg_dir: dict[str, Path] = {}
    for pkg in meta["packages"]:
        if pkg["id"] in workspace_ids:
            pkg_name[pkg["id"]] = pkg["name"]
            pkg_dir[pkg["id"]] = Path(pkg["manifest_path"]).parent.resolve()

    reverse_deps: dict[str, set[str]] = {pid: set() for pid in workspace_ids}
    for node in meta["resolve"]["nodes"]:
        if node["id"] not in workspace_ids:
            continue
        for dep in node["deps"]:
            if dep["pkg"] in workspace_ids:
                reverse_deps[dep["pkg"]].add(node["id"])

    repo_root = Path.cwd().resolve()
    dir_to_pkg = {d: pid for pid, d in pkg_dir.items()}

    touched: set[str] = set()
    workspace_wide = False
    for rel in files:
        path = (repo_root / rel).resolve()
        match: str | None = None
        for ancestor in [path, *path.parents]:
            if ancestor in dir_to_pkg:
                match = dir_to_pkg[ancestor]
                break
            if ancestor == repo_root:
                break
        if match is not None:
            touched.add(match)
            continue
        if rel in WORKSPACE_WIDE_FILES or any(
            rel.startswith(p) for p in WORKSPACE_WIDE_PREFIXES
        ):
            workspace_wide = True

    if workspace_wide:
        print("--workspace")
        return 0
    if not touched:
        print("")
        return 0

    needed = set(touched)
    queue = list(touched)
    while queue:
        cur = queue.pop()
        for parent in reverse_deps.get(cur, ()):
            if parent not in needed:
                needed.add(parent)
                queue.append(parent)

    print(" ".join(f"-p {pkg_name[pid]}" for pid in sorted(needed)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
