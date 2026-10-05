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
WASM target. Run commands from the repository root:

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

`consumer-dependencies.json` declares the compatible source archive range. Each
check selects its latest complete release and verifies the archive checksum.
The check applies the candidate app-core through a temporary Cargo override,
checks native and WASM targets and runs the effect-dispatch tests. For a local
consumer checkout, pass its explicit path, such as
`bash scripts/check-consumers.sh ../web`. The local browser repository has no
published consumer release yet.

Generate host bindings alone with `bash scripts/build-bindings.sh`; outputs go
under ignored `dist/`. Shell protocol **14** requires matching native bindings and
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

## Package releases

Our reusable crates are stored as `.crate` assets in this repository's GitHub
Releases. The `cargo-index` branch contains the Cargo sparse index; its entries
include immutable archive checksums. `.cargo/config.toml` registers the indexes.
Normal checks need only this repository's source. They refresh internal Cargo
versions before building; the committed lockfile supplies the initial third-party
selection rather than holding internal packages to an older release.

A successful `main` CI run starts the Release workflow. Release-plz calculates
versions and changelogs, and automation commits that metadata to `main`. The
entire CI workflow checks the version commit before any package is published.
Package archives, indexes and native bundles then publish from that exact commit;
there is no separate release PR. Concurrent changes to `main` are never overwritten.

Declare breaking changes in the feature PR, including the required minimum
versions in consumers. Release-plz uses commit messages and Rust API checks to
calculate the next version. To recover a failed publication, use **Re-run failed
jobs** on that Release run, retaining its verified commit even if `main` has
advanced. Dispatch **Release** on `main` to prepare current changes or resume a
current version commit. Existing versions and public archives remain immutable;
retries can complete unfinished drafts. A documentation-only change that does not alter packaged
contents does not create another package version.

Dependabot requires a secret reference for custom Cargo registries, including
public ones. Set the repository's Dependabot secret `PUBLIC_CARGO_REGISTRY_TOKEN`
to the literal value `anonymous`. This is a public marker, not an access token;
the GitHub indexes remain anonymously readable.

Every PR and main CI run resolves the latest compatible internal Cargo packages
and complete native/consumer releases before checking the code. Native and
consumer manifests declare Cargo-style version ranges, such as `^0.1.2`, instead
of fixed release tags and archive checksums. The resolver verifies published
checksums and records the selected versions in ignored
`target/released-dependencies.json`. All jobs in that CI run use this selection;
release verification, publication and native platform builds reuse it as well.

A new build of the same source commit can select newer dependencies. CI retains
its dependency record as an artifact, and releases include that record alongside
their packages. Release preparation incorporates the selected Cargo dependencies
in the version commit, so dependency changes can produce new binaries without a
separate dependency PR. Existing published package versions remain immutable.

The **Check latest released dependencies** workflow compares releases every 15
minutes, or on manual request, and starts ordinary main CI when its inputs have
changed. It creates no branch or PR. PR builds resolve immediately and do not
wait for that schedule. Failed selections remain visible in CI; rerun CI to retry
the same selection, or publish a fix to trigger a new check. Dependabot version
updates remain paused and do not participate in this internal dependency flow.

Keep consumer version requirements accurate when code starts using a new API.
A requirement of `^0.1.2` accepts `0.1.3`; adopting `0.2.0` requires an explicit
requirement change. Canonical `scripts/lint.sh` and `scripts/check.sh` also resolve
latest dependencies. For an individual local command, use:

```sh
python3 scripts/release_dependencies.py run -- cargo build --workspace --locked
```

Resolution uses the authenticated GitHub CLI (`gh`) to discover published assets.
Within one build, `--locked` keeps later commands on the selection that was just
resolved; it does not prevent the next build from selecting newer releases.


## Coordinated development

For ordinary local Rust work, add a temporary Cargo patch for the relevant
registry and pass it with `cargo --config /absolute/path/local.toml ...`.
Keep these overrides out of committed manifests and lockfiles. Full checks with
an unpublished producer can use `memos/scripts/check-integration.py` with
explicit `--producer` and `--consumer` checkout paths. It temporarily patches
Cargo, builds candidate native bundles when needed, runs the consumer's normal
check script and restores its dependency files. The manual **Unpublished package
integration** workflow in memos runs the same check for selected branches.
