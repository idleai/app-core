#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
if [ "$#" -eq 0 ]; then
    set -- web vscode-extension
fi

for consumer in "$@"; do
    case "$consumer" in
        web) package=idle-web ;;
        vscode-extension) package=idle-vscode-webview ;;
        *) echo "Unknown consumer: $consumer" >&2; exit 1 ;;
    esac
    consumer_root="$repo_root/../$consumer"
    if [ ! -f "$consumer_root/Cargo.toml" ]; then
        echo "Required consumer checkout is missing: $consumer_root" >&2
        exit 1
    fi
    (
        cd "$consumer_root"
        cargo check --workspace --all-targets --all-features --locked
        cargo test --locked -p "$package" --lib
        cargo check --locked -p "$package" --lib --all-features \
            --target wasm32-unknown-unknown
    )
done
