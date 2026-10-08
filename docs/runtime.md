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
| `protocol_version()` | Returns `15`; includes history, workspace, subscription, session, projection, resource, configuration and repository payloads. |
| `process_event(event)` | Encoded `Event` → encoded `EffectBatch`. |
| `handle_response(id, result)` | Encoded `HostInfoResponse`, `QueryResponse`, `WorkspaceResponse`, `SubscriptionResponse`, `SessionResponse`, `ProjectionResponse`, `ResourceResponse` or `ConfigurationResponse` → encoded `EffectBatch`. |
| `view()` | Encoded `ViewModel`. |

Use the generated codecs to encode and decode these bytes. Process every returned
batch, including follow-up effects. A `Render` effect asks for a fresh view and
needs no response. Return each other effect's result and request ID to the same
`AppCore` instance. Unknown, completed and render-only IDs are rejected.

Protocol 15 adds the indexed Activity view, exact current/retained selection,
window cursors, Find and independent editor/sidebar state. Regenerate Swift and
Kotlin types together with the native library. Earlier event and open-target
variant positions remain stable; the expanded view model requires the new version.
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
repository-to-chain bindings, member activity views and navigation. Hosts resolve
boxed workspace requests with `request.as_mut()`. Workspace selection wires the
logical chain into history; repository changes within a workspace retain it.

See [session integration](sessions.md) for owned/invited session state, explicit
logical history bindings, sharing and attributed runtime input. Production hosts
report unavailable runtime capabilities until connected; the optional
`session-fixtures` feature is for development and tests.

See [subscription integration](subscriptions.md) for connection lifetimes and
reconciliation. Domain effects use typed one-shot continuations and render notifications.

See [projection integration](projections.md) for shared `idle-protocol` inputs,
engine reads, controller mapping and the five typed destinations. Protocol 7 adds
`Event::Projections`, `Effect::Projection` and the root `projections` view. Hosts
must add the effect arm and regenerate matching codecs; the JSON coordination
protocol remains v1. Development fixtures require `projection-fixtures`.

See [resource integration](resources.md) for workspace-bound compute/providers,
model selection/installation and controller status. Protocol 8 adds
`Event::Resources`, `Effect::Resource` and the root `resources` view. Hosts must
handle the new effect and regenerate codecs with the native library. Runtime
actions remain unavailable until connected; `resource-fixtures` is development-only.
Protocol 9 adds `ResourceEvent::Restore` to recover persisted resource actions
after client destruction without submitting execution again. Regenerate the
payload codecs together with the native library.

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

Protocol 14 adds `ProjectionQuery::refresh_sources` and
`ProjectionEvent::Changed`. Explicit projection refreshes revalidate upstream
sources; background changes permit recent source reads and cannot replace a
queued explicit refresh. Regenerate native codecs together with the library.

See [configuration integration](configuration.md) for independent versioned settings
and agent-rule editors. Protocol 10 adds `Event::Configuration`,
`Effect::Configuration`, `ConfigurationResponse` and the root `configuration` view.
Hosts route loads and conditional saves through either coordination provider;
Offstage owns managed persistence and Evo owns rule enforcement.
