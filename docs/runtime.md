# Runtime and host integration

`app-core` keeps shared application state and behavior in Rust using Crux.
Reducers update state and request effects. The host runs those effects and renders
the resulting view. Each client owns an independent core.

```text
Client event → reducer → host effect → returned result → updated view
```

Run the [bootstrap example](../crates/app-core/examples/bootstrap.rs):

```sh
cargo run --locked -p app-core --example bootstrap
```

`Start` requests the host's name and version. The view changes from `Idle` to
`Loading`, then to `Ready(HostInfo)` or `Failed(EffectError)`. Send
`Bootstrap(Load)` to retry a failure. Repeated starts and loads already pending or
ready do nothing. `initialized` means the start event ran; `bootstrap` shows
whether the host request succeeded.

Rust hosts, including Dioxus web, use `Core` directly and return results with
`core.resolve(...)`. Dioxus compiles the core into its WASM app. Swift and Kotlin
hosts use BoltFFI's generated `AppCore` class. Facet generates their payload
types and bincode codecs.

| Rust method (camelCase in generated bindings) | Payloads |
| --- | --- |
| `protocol_version()` | Returns `4`; includes semantic history and workspace payloads. |
| `process_event(event)` | Encoded `Event` → encoded `EffectBatch`. |
| `handle_response(id, result)` | Encoded `HostInfoResponse`, `QueryResponse` or `WorkspaceResponse` → encoded `EffectBatch`. |
| `view()` | Encoded `ViewModel`. |

Use the generated codecs to encode and decode these bytes. Process every returned
batch, including follow-up effects. A `Render` effect asks for a fresh view and
needs no response. Return each other effect's result and request ID to the same
`AppCore` instance. Unknown, completed and render-only IDs are rejected.
Malformed results can be corrected and retried; a valid `Err` result becomes
failure state in the view.

The runnable [Swift example](../scripts/swift-smoke/main.swift) and
[Kotlin/JVM example](../scripts/kotlin-smoke/Main.kt) demonstrate this whole loop
using generated types, including failure and retry.

To add a domain module, follow the [bootstrap reducer](../crates/app-core/src/bootstrap.rs).
`module::Module` is Crux's `App` interface: each module defines its model, events,
effects, view model, `update` and `view`.

1. Add the module's model, event route and view to
   [the root app](../crates/app-core/src/app.rs). Use `map_event` to route completion
   events back through the root.
2. Define operations and results in [effects.rs](../crates/app-core/src/effects.rs),
   including their wire conversion and response validation. Derive `Facet` on
   wire types and register new result types in
   [codegen.rs](../crates/app-core/src/bin/codegen.rs).
3. Return commands and request a render when the view changes. Mark internal
   completion events with both `#[serde(skip)]` and `#[facet(skip)]`.
   Test success and failure, then regenerate the host bindings.

Use distinct Rust names for reflected domain event enums, then re-export them as
`Event` if desired. Facet 0.19's registry can conflate multiple nested enums named
`Event` despite rename attributes. The history and workspace modules use
`HistoryEvent` and `WorkspaceEvent` and verify their generated event/result
codecs in both native smoke tests.

See [history integration](history.md) for engine queries, operation records,
content and file comparisons. Rust hosts resolve boxed history requests with
`request.as_mut()`.

See [workspace integration](workspace.md) for both coordination modes,
repository-to-chain bindings, member/presence views and navigation. Hosts resolve
boxed workspace requests with `request.as_mut()`. Workspace selection wires the
logical chain into history; repository changes within a workspace retain it.

This foundation handles one-shot effects and render notifications. Subscription
and reconnect behavior belong to later domain work.

Build with `bash scripts/build-bindings.sh`; prerequisites are in
[the README](../README.md). The build replaces generated `dist/` contents.

| Output | Contents |
| --- | --- |
| `dist/native/` | Swift and Kotlin bindings, host libraries, runnable Swift smoke package. |
| `dist/types/` | Facet Swift and Kotlin payload types and codecs. |

Apple and Android packaging is configured in
[boltffi.toml](../crates/app-core-bindings/boltffi.toml). Linux checks exercise
Swift and Kotlin/JVM through JNI, and verify Rust WASM compilation. Android
device builds and execution still need the Android toolchain and separate tests.

Run `./scripts/lint.sh` for Rust checks, or `./scripts/check.sh` to also build,
generate bindings and run the host smoke tests. Binary payload layout depends on
field and variant order: coordinate incompatible changes with hosts and bump
`PROTOCOL_VERSION`. The Rust wire-layout test and foreign-language round trips
check codec compatibility.
