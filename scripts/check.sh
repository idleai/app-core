#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
bash scripts/lint.sh
cargo build --workspace --locked
cargo build --workspace --locked --target wasm32-unknown-unknown
cargo run --locked --example bootstrap
