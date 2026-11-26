#!/usr/bin/env bash
set -euo pipefail

cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."

cargo fmt --all --check
python3 scripts/check-comments.py
python3 scripts/check-file-size.py
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo clippy --locked -p sync2gether-web --target wasm32-unknown-unknown -- -D warnings
cargo test --locked --workspace --all-targets
cargo test --locked --workspace --all-targets --all-features
RUSTDOCFLAGS='-D warnings' cargo doc --locked --workspace --all-features --no-deps
