# idle-protocol

Versioned Rust types and JSON Schema for requests, responses and events exchanged
between Idle clients, Evo, and standalone or managed services.

Covers membership, Runner/Control sessions, resources, grants, Control leases and
recovery cursors. Requests carry contributor identity and retry keys; backend
receipt, runtime acceptance, ordering and completion are separate states.

Consumers depend directly on this crate. It has no dependency on the `app-core`
application, EditChain, private backends or cloud SDKs. Serde is the only default
dependency.

## Use

From a sibling repository's workspace:

```toml
[workspace.dependencies]
idle-protocol = { path = "../app-core/crates/idle-protocol", version = "0.1.0" }
```

In the consuming crate:

```toml
[dependencies]
idle-protocol.workspace = true
```

Import types from `idle_protocol::v1`. `SessionKind` is `Runner` or `Control`,
serialized as `"runner"` or `"control"`. Messages carry `api_version: "1"`.

## Schema and verification

The optional `schema` feature enables JSON Schema generation. From the repo root:

```sh
cargo run --locked -p idle-protocol --features schema \
  --example export_schema -- crates/idle-protocol/schemas/v1.json
./scripts/lint.sh
```

The lint suite checks wire fixture round trips and schema drift alongside the
workspace's Rust checks.

See the [v1 protocol reference](docs/v1.md) for compatibility, retries, authority,
fencing and recovery rules; [JSON Schema](schemas/v1.json) and
[fixtures](tests/fixtures) describe the wire format.
