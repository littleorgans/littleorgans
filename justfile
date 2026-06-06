set shell := ["bash", "-cu"]

# sccache caches every dependency compilation once and serves it to all three
# target dirs below (and across branch switches), so the per-tool split is cheap
# to fill. Local only: CI has no sccache, so this stays out of .cargo/config.toml.
export RUSTC_WRAPPER := "sccache"

# Per-tool CARGO_TARGET_DIR. clippy runs the clippy-driver, a different rustc
# fingerprint, so sharing target/ with build forces a full workspace recompile on
# every clippy<->build alternation (measured ~3min vs ~20s). nextest gets its own
# dir too: its dev-dep feature set differs from `cargo build`. CI never runs
# through this justfile, so it keeps using the default target/.
#
# Absolute (via justfile_directory()): a relative target dir is resolved against
# each cargo invocation's cwd, so a nested cargo run (e.g. an integration test
# spawning cargo from a crate dir) would scatter stray crates/<x>/target trees.
TARGET_CLIPPY := justfile_directory() / "target/clippy"
TARGET_BUILD := justfile_directory() / "target/build"
TARGET_NEXTEST := justfile_directory() / "target/nextest"

LILO_DEV_BIN := env("LILO_DEV_BIN", env("HOME") / ".cargo/bin/lilo")
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
    export CARGO_TARGET_DIR={{TARGET_BUILD}}
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
    CARGO_TARGET_DIR={{TARGET_BUILD}} cargo build --workspace --release

test *ARGS:
    #!/usr/bin/env bash
    set -euo pipefail
    export CARGO_TARGET_DIR={{TARGET_NEXTEST}}
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
    CARGO_TARGET_DIR={{TARGET_NEXTEST}} cargo test --workspace --doc

# Run the #[ignore]d lilo-db Postgres tests (opt-in; require a database).
# Set LILO_TEST_DATABASE_URL (or copy settings.example.toml to
# $LILO_HOME/settings.toml) first, e.g. with the local compose service:
#   docker compose up -d --wait postgres
#   LILO_TEST_DATABASE_URL=postgres://lilo:lilo@localhost:55432/lilo just test-db
test-db:
    CARGO_TARGET_DIR={{TARGET_NEXTEST}} cargo nextest run -p lilo-db --run-ignored all

lilo *ARGS:
    CARGO_TARGET_DIR={{TARGET_BUILD}} cargo run -p lilo --bin lilo -- {{ARGS}}

codegen *ARGS:
    CARGO_TARGET_DIR={{TARGET_BUILD}} cargo run -p xtask -- codegen {{ARGS}}

# Reclaim disk. Removes every Cargo target dir at once: the per-tool
# target/{build,clippy,nextest}, target/rust-analyzer, and the default target/
# that CI, bare `cargo`, and rust-analyzer still use. `cargo clean` only knows one
# CARGO_TARGET_DIR at a time, so it cannot do this. The next build/clippy/test
# refills dependency compiles from the sccache cache (~15s), not a cold build.
clean:
    rm -rf target

# Install

install: install-release

build-local:
    CARGO_TARGET_DIR={{TARGET_BUILD}} LILO_VERSION_INCLUDE_GIT_SHA=1 cargo build -p lilo --bin lilo --profile install-local

build-install-release:
    CARGO_TARGET_DIR={{TARGET_BUILD}} LILO_VERSION_INCLUDE_GIT_SHA=0 cargo build -p lilo --bin lilo --release

install-local: build-local
    @just _install-bin {{TARGET_BUILD}}/install-local/lilo

install-release: build-install-release
    @just _install-bin {{TARGET_BUILD}}/release/lilo

_install-bin src:
    @set -eu; \
    src="{{src}}"; case "$src" in /*) ;; *) src="$(pwd)/$src";; esac; \
    dest="{{LILO_DEV_BIN}}"; \
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
    CARGO_TARGET_DIR={{TARGET_CLIPPY}} cargo clippy --workspace --all-targets -- -D warnings

clippy-fix:
    CARGO_TARGET_DIR={{TARGET_CLIPPY}} cargo clippy --fix --workspace --all-targets --allow-dirty --allow-staged -- -D warnings

check-loc:
    bash scripts/check-loc-limit.sh

check-provenance:
    bash scripts/check-provenance.sh

check-seam:
    bash scripts/check-seam.sh

check-env:
    python3 scripts/check-env.sh --check

# Scope clippy to changed crates + reverse-dep closure. Run read-only clippy
# first because `cargo clippy --fix` uses a different fingerprint mode from
# read-only clippy and triggers a full workspace recompile on every invocation
# (~30-60s warm). When validation passes, the gate is sub-second warm. When
# it fails, fall back to --fix to auto-correct, then re-validate.
_clippy-incremental:
    #!/usr/bin/env bash
    set -euo pipefail
    export CARGO_TARGET_DIR={{TARGET_CLIPPY}}
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
# to changed crates + reverse deps. fmt / loc / provenance / seam always run
# workspace-wide because they are cheap and operate on raw files, not the
# Rust compile graph.
check: fmt _clippy-incremental fmt-check check-loc check-provenance check-seam check-env

# Full-workspace gate. Use before merging to main, in CI, or any time the
# scoping heuristic in scripts/changed-crates.sh might miss a regression
# surface (e.g. workspace-wide refactors, release-prep, manual audits).
# Mirrors the legacy `cargo fmt --all -- --check && clippy --workspace
# --all-targets && nextest run --workspace` chain.
regression:
    cargo fmt --all -- --check
    CARGO_TARGET_DIR={{TARGET_CLIPPY}} cargo clippy --workspace --all-targets -- -D warnings
    CARGO_TARGET_DIR={{TARGET_NEXTEST}} cargo nextest run --workspace
    bash scripts/check-loc-limit.sh
    bash scripts/check-provenance.sh
    bash scripts/check-seam.sh
    python3 scripts/check-env.sh --check
