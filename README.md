# app-core

Shared Rust application state for Idle. Crux reducers manage history, workspace
navigation, owned/invited sessions, resources, projections, settings/rules and subscription recovery. Hosts execute typed effects and render
views; storage, transports, inference and authorization enforcement stay with
their owning hosts and services.

| Crate | Purpose |
| --- | --- |
| [app-core](crates/app-core) | Shared events, reducers, effects and view models. |
| [app-core-bindings](crates/app-core-bindings) | BoltFFI bindings for Swift and Kotlin hosts. |

Check out `idleai/editchain` at `../editchain` and `idleai/host-tools` at
`../host-tools`. Host-tools owns the independent `idle-protocol`, `idle-history`
and native history service packages. Portable query/result types come from
`idle-history`; native effect adapters delegate reads to `idle-history-native`
with its service feature disabled. This workspace owns peer activity views,
branch invitation state and join preparation in `peer_activity`. Authorization
remains with coordination/runtime services. This workspace has no renderer or
VS Code dependency. Graph geometry lives in web-ui; platform actions remain in
the client host. [rust-toolchain.toml](rust-toolchain.toml) pins Rust and the
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

Check public API changes against sibling `web` and `vscode-extension` checkouts:

```sh
bash scripts/check-consumers.sh
```

This checks each native workspace and browser WASM target with locked dependencies
and runs the effect-dispatch tests. Every requested checkout is required. Pass
`web` or `vscode-extension` to check one consumer. CI checks the published extension;
the browser check currently runs locally until its repository is published.
CI pins sibling source revisions in `.github/workflows/ci.yml`; update those refs
together after checking the selected source combination locally. Referenced
commits must be published in their repositories before remote CI can fetch them.

Generate host bindings alone with `bash scripts/build-bindings.sh`; outputs go
under ignored `dist/`. Shell protocol **13** requires matching native bindings and
payload codecs. Rust and Dioxus/WASM clients depend directly on `app-core`.

Integration guides:

- [Runtime and native bindings](docs/runtime.md) — host effect loop, codecs and packaging.
- [History](docs/history.md) — selection, search, paging and recorded content.
- [Workspaces](docs/workspace.md) — repository bindings, members and peer activity.
- [Sessions](docs/sessions.md) — creation, sharing, explicit history bindings and attributed input.
- [Resources](docs/resources.md) — compute/providers, model actions, progress and controller status.
- [Settings and agent rules](docs/configuration.md) — versioned documents, drafts, conflicts and save feedback.
- [Projections](docs/projections.md) — shared inputs, activity/task/error/triage/need-input views and controller mapping.
- [Subscriptions](docs/subscriptions.md) — joins, reconnects and snapshot reconciliation.
- [Coordination protocol](https://github.com/idleai/host-tools/tree/main/crates/idle-protocol) — public contracts and JSON Schema.

Contributor rules are in [AGENTS.md](AGENTS.md); dependency policy and its documented
exceptions are in [deny.toml](deny.toml).

See [repository integration](docs/repository.md) for Git/GitHub reads, recorded
session selection and the shell protocol 14 boundary.
