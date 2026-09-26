# app-core

`app-core` provides Idle's shared application logic. `idle-protocol` defines the
versioned requests, responses and events exchanged by clients, runtimes and
services.

Both crates live under `crates/` in a Cargo workspace defined by the root
`Cargo.toml`. Application code and examples are in `crates/app-core/{src,examples}/`.
Run build and check commands from the repository root.

Module ownership, current behavior and build instructions follow. Reserved
application modules are placeholders for planned features.

Owns shared application behavior, never rendering, filesystem capture, inference
or authorization enforcement. Crux 0.20 is linked through a single bootstrap
event/render/view slice. f21 supplies real effect coordination and native/FFI
bindings later; the current `cdylib`/`staticlib` targets are not a finished FFI.

Rust 1.97.0 is selected by `rust-toolchain.toml`. Cargo installs the specified
toolchain/targets on first use; lockfiles are tracked. Install the dependency
policy checker once:

```sh
cargo install cargo-deny --locked --version 0.20.2
```

`./scripts/lint.sh` is the canonical Rust quality gate, shared by local checks and
CI: formatting, locked checks/Clippy/tests with all features, doctests, Rustdoc
and cargo-deny. Browser-facing libraries also run checks and Clippy for
`wasm32-unknown-unknown`. Every package inherits EditChain's strict workspace
Rust, Clippy and Rustdoc rules and its thresholds in `clippy.toml`.
`./scripts/check.sh` runs that gate followed by this repo's builds/packaging.

```sh
bash scripts/check.sh
cargo run --locked -p app-core --example bootstrap
cargo build --workspace --locked --target wasm32-unknown-unknown
```

No sibling repository is required to build this repository.

| Boundary | Owner after f1 |
| --- | --- |
| Root manifest, exports, bootstrap/runtime and bindings | f21/crux-runtime |
| `crates/idle-protocol`, its manifest, exports and schemas | f20/coordination-contracts |
| `crates/app-core/src/workspace.rs` | f22/workspace-state |
| `crates/app-core/src/history.rs` | f23/history-state |
| `crates/app-core/src/sessions.rs` | f24/session-state |
| `crates/app-core/src/projections.rs` | f25/projection-state |
| `crates/app-core/src/resources.rs` | f26/resource-state |
| `crates/app-core/src/configuration.rs` | f27/settings-rules-state |
| `crates/app-core/src/subscriptions.rs` | f28/subscription-state |

The [idle-protocol crate](crates/idle-protocol/README.md) publishes the
`idle_protocol::v1` Rust API and a checked-in
[v1 JSON Schema](crates/idle-protocol/schemas/v1.json). Runtimes and
standalone/managed coordination adapters can consume membership, session, resource,
grant, Control lease and recovery contracts without depending on Crux.
Authenticated contributor attribution and retry identities are shared; coordination
receipt remains separate from runtime acceptance, input order and completion.
Service implementations and runtime enforcement remain with their consumers.

`idle-protocol` is an independent package in this Cargo workspace. Applications,
runtimes and managed coordination providers depend directly on it; importing it
does not import the `app-core` application crate. The public protocol and OSS
stack must remain independent of private backend source. EditChain remains the
history engine; workspace coordination contracts are owned here.

Reuse semantic state from EditChain's
`crates/editchain-history-renderer/src/app/`, viewer portions of
`editchain-node/src/history/` and `editchain-protocol/src/`, and extension sharing
flows during their named feature sessions. Keep DOM plans, pixel geometry and
native bridges with their owners. f1 copies none of those implementations.

The dependency policy in `deny.toml` includes one explicit maintenance exception:
[RUSTSEC-2025-0141](https://rustsec.org/advisories/RUSTSEC-2025-0141.html), for
Crux 0.20's mandatory `bincode` 1.3.3 dependency. Remove it when Crux migrates
serialization. Other advisories remain checked. Crux's optional macro feature
is disabled, removing its unmaintained `proc-macro-error` dependency.
