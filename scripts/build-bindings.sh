#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

# Keep the CLI aligned with the crate's pinned wasm-bindgen version.
test "$(wasm-bindgen --version)" = "wasm-bindgen 0.2.127"
cargo build --locked -p app-core-bindings
cargo build --locked -p app-core-bindings --target wasm32-unknown-unknown

case "$(uname -s)" in
    Linux) library=libapp_core_bindings.so ;;
    Darwin) library=libapp_core_bindings.dylib ;;
    *) echo "Native binding packaging currently supports Linux and macOS hosts" >&2; exit 1 ;;
esac

mkdir -p dist/native dist/wasm dist/wasm-node
cargo run --locked -p app-core-bindings --features bindgen --bin uniffi-bindgen -- \
    generate --library "target/debug/$library" \
    --language swift --language kotlin --language python --no-format --out-dir dist/native
cp "target/debug/$library" target/debug/libapp_core_bindings.a dist/native/
cp crates/app-core/schemas/shell-v1.json dist/
wasm-bindgen --target web --out-dir dist/wasm --out-name app_core \
    target/wasm32-unknown-unknown/debug/app_core_bindings.wasm
wasm-bindgen --target nodejs --out-dir dist/wasm-node --out-name app_core \
    target/wasm32-unknown-unknown/debug/app_core_bindings.wasm
