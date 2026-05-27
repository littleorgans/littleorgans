set shell := ["bash", "-cu"]

LILO_LOCAL_BIN := env("LILO_LOCAL_BIN", env("HOME") / ".cargo/bin/lilo")
BASE_REF := env("BASE_REF", "main")

default:
    @just --list

# Build, test, run
# `build` and `test` scope to changed crates + reverse-dep closure via
# scripts/changed-crates.sh (default base ref: `main`, override with
# BASE_REF=...). Falls back to `--workspace` on workspace-wide changes
# (root Cargo.toml, rust-toolchain.toml, .cargo/*). Use `just regression`
# for the unconditional full-workspace gate.

build:
    #!/usr/bin/env bash
    set -euo pipefail
    flags="$(scripts/changed-crates.sh {{BASE_REF}})"
    if [[ -z "$flags" ]]; then
        echo "[build] no relevant changes vs {{BASE_REF}}; nothing to compile."
        exit 0
    fi
    if [[ "$flags" == "--workspace" ]]; then
        echo "[build] workspace-wide change; cargo build --workspace."
        cargo build --workspace
    else
        echo "[build] scoped:$(echo "$flags" | tr -s ' ' | sed 's/-p / /g')"
        cargo build $flags
    fi

release-build:
    cargo build --workspace --release

test *ARGS:
    #!/usr/bin/env bash
    set -euo pipefail
    flags="$(scripts/changed-crates.sh {{BASE_REF}})"
    if [[ -z "$flags" ]]; then
        echo "[test] no relevant changes vs {{BASE_REF}}; nothing to run."
        exit 0
    fi
    if [[ "$flags" == "--workspace" ]]; then
        echo "[test] workspace-wide change; cargo nextest run --workspace."
        cargo nextest run --workspace {{ARGS}}
    else
        echo "[test] scoped:$(echo "$flags" | tr -s ' ' | sed 's/-p / /g')"
        cargo nextest run $flags {{ARGS}}
    fi

test-doc:
    cargo test --workspace --doc

lilo *ARGS:
    cargo run -p lilo --bin lilo -- {{ARGS}}

# Install

install: install-release

build-local:
    LILO_VERSION_INCLUDE_GIT_SHA=1 cargo build -p lilo --bin lilo --profile install-local

build-install-release:
    LILO_VERSION_INCLUDE_GIT_SHA=0 cargo build -p lilo --bin lilo --release

install-local: build-local
    @just _install-bin target/install-local/lilo

install-release: build-install-release
    @just _install-bin target/release/lilo

_install-bin src:
    @set -eu; \
    src="$(pwd)/{{src}}"; \
    dest="{{LILO_LOCAL_BIN}}"; \
    case "$dest" in /*) ;; *) dest="$(pwd)/$dest";; esac; \
    if [ "$src" = "$dest" ]; then \
        echo "Built $src"; \
    else \
        mkdir -p "$(dirname "$dest")"; \
        install -m 755 "$src" "$dest"; \
        echo "Installed $dest"; \
    fi; \
    "$dest" --version

# Lint and check

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

# Workspace-wide clippy. Use only when changed-crates fallback fires or via
# `just regression`. Individual gate runs go through `_clippy-incremental`.
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

clippy-fix:
    cargo clippy --fix --workspace --all-targets --allow-dirty --allow-staged -- -D warnings

check-loc:
    bash scripts/check-loc-limit.sh

check-provenance:
    bash scripts/check-provenance.sh

# Scope clippy to changed crates + reverse-dep closure. Run read-only clippy
# first because `cargo clippy --fix` uses a different fingerprint mode from
# read-only clippy and triggers a full workspace recompile on every invocation
# (~30-60s warm). When validation passes, the gate is sub-second warm. When
# it fails, fall back to --fix to auto-correct, then re-validate.
_clippy-incremental:
    #!/usr/bin/env bash
    set -euo pipefail
    flags="$(scripts/changed-crates.sh {{BASE_REF}})"
    if [[ -z "$flags" ]]; then
        echo "[clippy] no relevant changes vs {{BASE_REF}}; skipping."
        exit 0
    fi
    if [[ "$flags" == "--workspace" ]]; then
        scope_label="workspace-wide"
        scope_flags=(--workspace)
    else
        scope_label="scoped:$(echo "$flags" | tr -s ' ' | sed 's/-p / /g')"
        scope_flags=($flags)
    fi
    echo "[clippy] $scope_label (read-only)"
    if cargo clippy "${scope_flags[@]}" --all-targets -- -D warnings; then
        exit 0
    fi
    echo "[clippy] lint failures; running --fix"
    cargo clippy --fix "${scope_flags[@]}" --all-targets --allow-dirty --allow-staged -- -D warnings
    echo "[clippy] re-validating after --fix"
    cargo clippy "${scope_flags[@]}" --all-targets -- -D warnings

# Pre-commit gate. Incremental by default; scopes clippy + clippy --fix
# to changed crates + reverse deps. fmt / loc / provenance always run
# workspace-wide because they are cheap and operate on raw files, not the
# Rust compile graph.
check: fmt _clippy-incremental fmt-check check-loc check-provenance

# Full-workspace gate. Use before merging to main, in CI, or any time the
# scoping heuristic in scripts/changed-crates.sh might miss a regression
# surface (e.g. workspace-wide refactors, release-prep, manual audits).
# Mirrors the legacy `cargo fmt --all -- --check && clippy --workspace
# --all-targets && nextest run --workspace` chain.
regression:
    cargo fmt --all -- --check
    bash scripts/check-loc-limit.sh
    bash scripts/check-provenance.sh
    cargo clippy --workspace --all-targets -- -D warnings
    cargo nextest run --workspace
