# app-core

Shared Rust application state for Idle. Crux reducers manage history, workspace
navigation, owned/invited sessions, resources, projections, settings/rules and subscription recovery. Hosts execute typed effects and render
views; storage, transports, inference and authorization enforcement stay with
their owning hosts and services.

| Crate | Purpose |
| --- | --- |
| [app-core](crates/app-core) | Shared events, reducers, effects and view models. |
| [app-core-bindings](crates/app-core-bindings) | BoltFFI bindings for Swift and Kotlin hosts. |

Cargo downloads the versions and checksums in `Cargo.lock` from producer-owned
GitHub releases. Host-tools owns the independent `idle-protocol`, `idle-history`
and native history service packages. Portable query/result types come from
`idle-history`; native effect adapters delegate reads to `idle-history-native`
with its service feature disabled. This workspace owns peer activity views,
branch invitation state and join preparation in `peer_activity`. Authorization
remains with coordination/runtime services. This workspace has no renderer or
VS Code dependency. Graph geometry lives in web-ui; platform actions remain in
the client host. [rust-toolchain.toml](rust-toolchain.toml) pins Rust and the
WASM target. Install Python 3.12+ and authenticate the GitHub CLI (`gh`) for
release downloads. Run commands from the repository root:

```sh
cargo install cargo-deny --locked --version 0.20.2
cargo run --locked -p app-core --example bootstrap
./scripts/lint.sh
```

For full checks, also install Swift 6.2+, a C compiler, JDK 21+ (`JAVA_HOME` set)
and Kotlin 2.2.0 (`kotlinc` on `PATH`), then run:

```sh
cargo install boltffi_cli --locked --version 0.31.0
./scripts/check.sh
```

The full check runs Rust lint/tests, native and WASM builds, binding generation,
and Swift/Kotlin round trips through the native library. Apple/Android device
builds require their platform SDKs separately.

Check public API changes against the released extension consumer:

```sh
bash scripts/check-consumers.sh
```

See [consumer compatibility](docs/packaging.md#consumer-compatibility) for
archive selection and checking a local consumer checkout.

Generate host bindings alone with `bash scripts/build-bindings.sh`; outputs go
under ignored `dist/`. Shell protocol **15** requires matching native bindings and
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
session selection and the shell protocol 15 boundary.

## Package releases

Checks select the latest compatible internal releases and reuse that selection
through testing and packaging. Rebuilding a commit can select newer versions.
Successful main CI starts automatic crate publication. See
[packaging and releases](docs/packaging.md) for the registry, workflow, retries and
testing unpublished dependencies.
