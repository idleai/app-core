#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
bash scripts/lint.sh
cargo build --workspace --locked
cargo build --workspace --locked --target wasm32-unknown-unknown
cargo run --locked -p app-core --example bootstrap
bash scripts/build-bindings.sh
swift run --package-path dist/native SwiftSmoke
bash scripts/smoke-kotlin.sh
