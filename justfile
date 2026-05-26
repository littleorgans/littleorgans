set shell := ["bash", "-cu"]

LILO_LOCAL_BIN := env("LILO_LOCAL_BIN", env("HOME") / ".cargo/bin/lilo")

default:
    @just --list

# Build, test, run

build:
    cargo build --workspace

release-build:
    cargo build --workspace --release

test *ARGS:
    cargo nextest run --workspace {{ARGS}}

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

clippy:
    cargo clippy --workspace --all-targets -- -D warnings

clippy-fix:
    cargo clippy --fix --workspace --all-targets --allow-dirty --allow-staged -- -D warnings

check-loc:
    bash scripts/check-loc-limit.sh

check: fmt clippy-fix fmt-check check-loc clippy
