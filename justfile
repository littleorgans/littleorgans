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
