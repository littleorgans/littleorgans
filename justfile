set shell := ["bash", "-cu"]

LILO_LOCAL_BIN := env("LILO_LOCAL_BIN", env("HOME") / ".cargo/bin/lilo")

default:
    @just --list

fmt:
    cargo fmt --all

fmt-check:
    cargo fmt --all -- --check

clippy:
    cargo clippy --workspace --all-targets -- -D warnings

clippy-fix:
    cargo clippy --fix --workspace --all-targets --allow-dirty --allow-staged -- -D warnings

build:
    cargo build --workspace

test:
    cargo test --workspace

check-loc:
    bash scripts/check-loc-limit.sh

check: fmt clippy-fix fmt-check check-loc clippy
