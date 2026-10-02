# app-core

Shared Rust application state for Idle. Crux reducers manage history, workspace
navigation, owned/invited sessions, resources, projections, settings/rules and subscription recovery. Hosts execute typed effects and render
views; storage, transports, inference and authorization enforcement stay with
their owning hosts and services.

| Crate | Purpose |
| --- | --- |
| [app-core](crates/app-core) | Shared events, reducers, effects and view models. |
| [app-core-bindings](crates/app-core-bindings) | BoltFFI bindings for Swift and Kotlin hosts. |
| [idle-protocol](crates/idle-protocol) | Versioned coordination contracts, independent of Crux. |
| [idle-history](crates/idle-history) | Portable history and connection state shared with the legacy EditChain viewer. |

Check out `idleai/editchain` at `../editchain` and `idleai/web-ui` at `../web-ui`.
The engine workspace also requires web-ui's shared history geometry manifest
when Cargo loads its dependencies. [rust-toolchain.toml](rust-toolchain.toml) pins Rust and the
WASM target. Run commands from the repository root:

```sh
cargo install cargo-deny --locked --version 0.20.2
cargo run --locked -p app-core --example bootstrap
./scripts/lint.sh
```

For full checks, also install Swift 6.2+, a C compiler, JDK 21+ (`JAVA_HOME` set)
and Kotlin 2.2.0 (`kotlinc` on `PATH`), then run:

```sh
cargo install boltffi_cli --locked --version 0.29.3
./scripts/check.sh
```

The full check runs Rust lint/tests, native and WASM builds, binding generation,
and Swift/Kotlin round trips through the native library. Apple/Android device
builds require their platform SDKs separately.

Generate host bindings alone with `bash scripts/build-bindings.sh`; outputs go
under ignored `dist/`. Shell protocol **11** requires matching native bindings and
payload codecs. Rust and Dioxus/WASM clients depend directly on `app-core`.

Integration guides:

- [Runtime and native bindings](docs/runtime.md) — host effect loop, codecs and packaging.
- [History](docs/history.md) — selection, search, paging and recorded content.
- [Workspaces](docs/workspace.md) — repository bindings, members and presence.
- [Sessions](docs/sessions.md) — creation, sharing, explicit history bindings and attributed input.
- [Resources](docs/resources.md) — compute/providers, model actions, progress and controller status.
- [Settings and agent rules](docs/configuration.md) — versioned documents, drafts, conflicts and save feedback.
- [Projections](docs/projections.md) — shared inputs, activity/task/error/triage/need-input views and controller mapping.
- [Subscriptions](docs/subscriptions.md) — joins, reconnects and snapshot reconciliation.
- [Coordination protocol](crates/idle-protocol/README.md) — public contracts and JSON Schema.

Contributor rules are in [AGENTS.md](AGENTS.md); dependency policy and its documented
exceptions are in [deny.toml](deny.toml).
