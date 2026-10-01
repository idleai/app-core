# app-core

`app-core` provides Idle's shared application logic. `app-core-bindings` exposes
that logic to Swift and Kotlin hosts. `idle-protocol` defines the versioned
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
interface, binary shell protocol, and extension points. BoltFFI generates the
native method bindings. Facet generates Swift and Kotlin payload types and bincode
codecs. The shell exposes `process_event`,
`handle_response`, `view`, and `protocol_version` (camelCase in generated hosts).
Protocol **4** adds workspace navigation, membership and presence events, effects
and views alongside semantic history. Hosts must regenerate their bindings and
use the generated codecs.

The [history integration guide](docs/history.md) covers stable selection, literal
search, filters, disclosure, paging, cached operation records and content replay.
Rust native hosts can execute its effects through `history::engine::execute`;
WASM hosts use the same portable requests through their own engine connection.
`idle-history` holds the small portable contracts still consumed by the legacy
EditChain viewer during its migration. Graph geometry, scrolling and rendering
remain client responsibilities.

The [workspace integration guide](docs/workspace.md) covers standalone/managed
selection, explicit repository-to-chain bindings, reusable resource attachments,
members, expiring presence, navigation and loading/error states. Workspace
selection passes only the logical chain reference to history. Hosts execute
coordination reads through their configured adapters using the published
`idle-protocol` snapshot projection.

Rust 1.97.0 is selected by `rust-toolchain.toml`. Cargo installs the specified
toolchain/targets on first use; lockfiles are tracked. Install the binding
generator and dependency policy checker once:

```sh
cargo install cargo-deny --locked --version 0.20.2
cargo install boltffi_cli --locked --version 0.29.3
```

`./scripts/lint.sh` is the canonical Rust quality gate, shared by local checks and
CI: formatting, locked checks/Clippy/tests with all features, doctests, Rustdoc
and cargo-deny. Browser-facing libraries also run checks and Clippy for
`wasm32-unknown-unknown`. Every package inherits EditChain's strict workspace
Rust, Clippy and Rustdoc rules and its thresholds in `clippy.toml`.
`./scripts/check.sh` runs that gate followed by native/WASM builds, the Rust
example, binding generation and actual foreign-language round trips through the
native library from Swift and Kotlin/JVM. These tests compile the generated
bindings and payload codecs, then cover invalid inputs, request IDs, host failures,
retry and independent clients. The JVM test also compiles BoltFFI's generated JNI
bridge and runs with `-Xcheck:jni`.

Host smoke prerequisites: Swift 6.2+, a C compiler, JDK 21+ (set `JAVA_HOME` to
the JDK directory) and the [Kotlin 2.2.0 compiler](https://github.com/JetBrains/kotlin/releases/tag/v2.2.0)
(`kotlinc` on `PATH`). CI installs these on Linux. No Android SDK, NDK, emulator,
Node.js or npm is required.

Generated bindings, payload types and native libraries are written under ignored
`dist/`. Generate them with `bash scripts/build-bindings.sh` on Linux or macOS.
Then run `swift run --package-path dist/native SwiftSmoke` and
`bash scripts/smoke-kotlin.sh` to exercise the bindings. Rust callers, including
Dioxus web, depend directly on `app-core` and compile it into their own WASM app.

[boltffi.toml](crates/app-core-bindings/boltffi.toml) configures Apple and Android
packaging. On hosts with the relevant SDKs, run `boltffi --cargo-arg=--locked
pack apple` or `pack android` from `crates/app-core-bindings/`; include the
corresponding Facet package from `dist/types/` in the shell. Device builds are
not part of the current Linux checks. The local native smoke build uses the same
BoltFFI expansion mode as Apple packaging.

[codegen.rs](crates/app-core/src/bin/codegen.rs) invokes Crux 0.20's Facet backend
directly: Crux's convenience feature also enables the deprecated macro
dependency rejected by our dependency policy. Generate payload types alone with
`cargo run --locked -p app-core --features typegen --bin codegen -- dist/types`.

```sh
bash scripts/check.sh
cargo run --locked -p app-core --example bootstrap
cargo build --workspace --locked --target wasm32-unknown-unknown
```

Check out `idleai/editchain` beside this repository as `../editchain`. Local path
dependencies use its core schema and engine queries; CI recreates the same layout.
The engine host adapter is native-only; WASM compiles the schema and pure reducer.

| Boundary | Owner after f1 |
| --- | --- |
| Root manifest, exports, bootstrap/runtime, `app-core-bindings` and shell payload types | f21/crux-runtime; f22 owns workspace composition and shell-v4 additions |
| `crates/idle-protocol`, its manifest, exports and schemas | f20/coordination-contracts |
| `crates/app-core/src/workspace{.rs,/}`, its app-core manifest dependency and host wiring | f22/workspace-state |
| `crates/app-core/src/history{.rs,/}`, `crates/idle-history` | f23/history-state |
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

History state adapts the selection, find, cache and disclosure behavior from
EditChain's `editchain-history-renderer::app`. The old renderer delegates semantic
selection to `idle-history`; node/protocol preview adapters also use that package.
The old viewer's coordinate-based caches, disclosure and find adapters remain
until the graph/detail consumers switch. Subscription/reconnect and extension
sharing flows remain with their named feature owners.

The dependency policy in `deny.toml` includes one explicit maintenance exception:
[RUSTSEC-2025-0141](https://rustsec.org/advisories/RUSTSEC-2025-0141.html), for
Crux 0.20's mandatory `bincode` 1.3.3 dependency. Remove it when Crux migrates
serialization. Other advisories remain checked. Crux's optional macro feature
is disabled, removing its unmaintained `proc-macro-error` dependency.

Consumers handle `Effect::HostInfo`, boxed `Effect::History` and boxed
`Effect::Workspace` requests, and include `bootstrap`, `history` and `workspace`
in `ViewModel` literals (or use `..Default::default()`).
`initialized` still means the start event was processed; readiness is represented
by `view.bootstrap`. Linkable native artifacts come from `app-core-bindings`,
while `app-core` is the Rust library used by native and WASM clients. Domain modules
reserved for f24–f28 remain with their feature owners.
