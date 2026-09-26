# app-core

`app-core` provides Idle's shared application logic. `app-core-bindings` exposes
that logic to native and JavaScript hosts. `idle-protocol` defines the versioned
coordination requests, responses and events exchanged by clients, runtimes and
services, independently of the UI runtime.

The crates live under `crates/` in a Cargo workspace defined by the root
`Cargo.toml`. Application code and examples are in `crates/app-core/{src,examples}/`.
Run build and check commands from the repository root.

Module ownership, current behavior and build instructions follow. Reserved
application modules are placeholders for planned features.

Owns shared application behavior, never rendering, filesystem capture, inference
or authorization enforcement. Crux 0.20 composes domain reducers and drives typed
host effects. A bootstrap reducer demonstrates `Start` → host information request
→ returned success/error → updated typed view. Each client has an independent core.

The [runtime and host integration guide](docs/runtime.md) defines the module
interface, JSON shell protocol, and extension points. Native hosts use generated
UniFFI bindings; JavaScript hosts use a wasm-bindgen class. Both expose
`process_event`, `handle_response`, `view`, and `protocol_version`, and use the
same [versioned shell schema](crates/app-core/schemas/shell-v1.json).

Rust 1.97.0 is selected by `rust-toolchain.toml`. Cargo installs the specified
toolchain/targets on first use; lockfiles are tracked. Install the dependency
policy checker once:

```sh
cargo install cargo-deny --locked --version 0.20.2
cargo install wasm-bindgen-cli --locked --version 0.2.127
```

`./scripts/lint.sh` is the canonical Rust quality gate, shared by local checks and
CI: formatting, locked checks/Clippy/tests with all features, doctests, Rustdoc
and cargo-deny. Browser-facing libraries also run checks and Clippy for
`wasm32-unknown-unknown`. Every package inherits EditChain's strict workspace
Rust, Clippy and Rustdoc rules and its thresholds in `clippy.toml`.
`./scripts/check.sh` runs that gate followed by native/WASM builds, the Rust
example, binding generation and actual foreign-language round trips through the
native library and WASM module. Packaging requires Node.js (CI uses 22) and Python
3. Generated Swift, Kotlin and Python wrappers, native libraries and web/Node WASM
bundles are written under ignored `dist/`. Generate them alone with
`bash scripts/build-bindings.sh` on Linux or macOS. Rust callers only need
`app-core`; FFI dependencies live in the separate bindings crate.

```sh
bash scripts/check.sh
cargo run --locked -p app-core --example bootstrap
cargo build --workspace --locked --target wasm32-unknown-unknown
```

No sibling repository is required to build this repository.

| Boundary | Owner after f1 |
| --- | --- |
| Root manifest, exports, bootstrap/runtime, `app-core-bindings` and shell schema | f21/crux-runtime |
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

Scaffold consumers must handle the new `Effect::HostInfo` operation and include
`bootstrap` when constructing a `ViewModel` literal (or use `..Default::default()`).
`initialized` still means the start event was processed; readiness is represented
by `view.bootstrap`. Linkable native/WASM artifacts now come from
`app-core-bindings`, while `app-core` is the Rust library. Domain modules reserved
for f22–f28 remain with their feature owners.
