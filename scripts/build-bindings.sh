#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

# Match the macro and CLI versions.
test "$(boltffi --version)" = "boltffi 0.31.0"

# dist is entirely generated; do not leave obsolete bindings beside new ones.
rm -rf dist
mkdir -p dist/native/swift-smoke
cargo run --locked -p app-core --features typegen --bin codegen -- dist/types

(
    cd crates/app-core-bindings
    boltffi --cargo-arg=--locked generate swift
    boltffi --cargo-arg=--locked generate kotlin
)

# BoltFFI's generated bindings use its IR expansion ABI. This is the same
# expansion that `boltffi pack apple` uses, built for the local smoke-test host.
BOLTFFI_BINDING_EXPANSION=1 \
BOLTFFI_BINDING_EXPANSION_ROOT="$PWD/crates/app-core-bindings" \
BOLTFFI_BINDING_EXPANSION_SOURCE="$PWD/crates/app-core-bindings/src/lib.rs" \
BOLTFFI_BINDING_EXPANSION_SURFACE=native \
BOLTFFI_BINDING_METADATA_FEATURES= \
cargo rustc --locked -p app-core-bindings --lib -- --cfg boltffi_binding_expansion

case "$(uname -s)" in
    Linux) library=libapp_core_bindings.so ;;
    Darwin) library=libapp_core_bindings.dylib ;;
    *) echo "Native binding packaging currently supports Linux and macOS hosts" >&2; exit 1 ;;
esac

cp "target/debug/$library" target/debug/libapp_core_bindings.a dist/native/
cp scripts/swift-smoke/Package.swift dist/native/
cp scripts/swift-smoke/main.swift dist/native/swift-smoke/
cat > dist/native/swift/module.modulemap <<'EOF'
module AppCoreBindingsFFI {
    header "boltffi.h"
    link "app_core_bindings"
    export *
}
EOF
