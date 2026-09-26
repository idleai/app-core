# Runtime and host integration

`app-core` keeps shared application state and behavior in Rust using Crux.
Reducers update state and request effects. The host runs those effects and renders
the resulting view. Each client owns an independent core.

The flow is:

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

Rust hosts use `Core` directly and return results with `core.resolve(...)`.
Native and JavaScript hosts use `AppCore` from `app-core-bindings`. Its methods
exchange UTF-8 JSON bytes using the [shell schema](../crates/app-core/schemas/shell-v1.json):

| Method | Purpose |
| --- | --- |
| `protocol_version()` | Check that the host supports protocol version `1`. |
| `process_event(event)` | Send a client event and receive effect requests. |
| `handle_response(id, result)` | Return an effect result and receive follow-up requests. |
| `view()` | Read the current view model. |

Process every returned effect, including follow-up batches. A `render` effect asks
for a fresh view and needs no response. For other effects, return the result and
request ID to the same `AppCore` instance. IDs belong only to that live instance.
Unknown, completed and render-only IDs are rejected. Malformed results can be
corrected and retried; a valid `Err` result becomes failure state in the view.

For complete host examples, see [native Python](../scripts/smoke-native.py) and
[JavaScript/WASM](../scripts/smoke-wasm.cjs). The bindings expose byte buffers;
the schema describes the event, effect, result and view payloads.

To add a domain module, follow the [bootstrap reducer](../crates/app-core/src/bootstrap.rs).
`module::Module` is Crux's `App` interface: each module defines its model, events,
effects, view model, `update` and `view`.

1. Add the module's model, event route and view to
   [the root app](../crates/app-core/src/app.rs). Use `map_event` to route its
   completion events back through the root.
2. Define typed operations and results in
   [effects.rs](../crates/app-core/src/effects.rs), including their wire conversion
   and response validation. The host provides an adapter for each operation.
3. Return commands from the reducer and request a render when its view changes.
   Mark internal completion events `#[serde(skip)]` so hosts return results through
   request IDs. Add wire types to `ShellContract`, regenerate the schema and test
   success and failure.

This foundation handles one-shot effects and render notifications. Subscription
and reconnect behavior belong to later domain work.

Build the bindings with `bash scripts/build-bindings.sh` (Linux or macOS).
[The README](../README.md) lists prerequisites.

| Output | Contents |
| --- | --- |
| `dist/native/` | UniFFI Swift, Kotlin and Python wrappers, plus host libraries. |
| `dist/wasm/` | Browser JavaScript, TypeScript method declarations and WASM. |
| `dist/wasm-node/` | Node.js wrapper and WASM used by the smoke test. |
| `dist/shell-v1.json` | Shared payload schema. |

UniFFI supports later iOS/Swift and Android/Kotlin clients. Those apps still need
platform builds, library linking, UI code and host effect adapters. The current
checks exercise the native ABI and WASM; mobile builds and devices are untested.

Run `./scripts/lint.sh` for Rust checks, or `./scripts/check.sh` to also build,
generate bindings and run the native/WASM smoke tests. After changing wire types,
regenerate the schema and coordinate incompatible changes with host consumers:

```sh
cargo run --locked -p app-core --features schema --example export_shell_schema -- \
  crates/app-core/schemas/shell-v1.json
```
