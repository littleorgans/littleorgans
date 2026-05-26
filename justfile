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

build:
    cargo build --workspace

test:
    cargo test --workspace

check-loc:
    ./scripts/check-loc-limit.sh

check: fmt-check clippy check-loc
