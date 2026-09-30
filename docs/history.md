# Semantic history

`Event::History(history::Event)` drives an independent Crux history model for each
client. Effects carry a logical chain binding and a typed `history::QueryAction`.
The host resolves that binding, executes the query and resolves the request. The
reducer does no filesystem, transport, DOM, scrolling or graph-layout work.

```rust,no_run
use app_core::{Core, Effect, Event, history};
use editchain_engine::queries::ChainQueries;

# fn example(path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
let core = Core::new();
let mut queries = ChainQueries::open(path)?;
for effect in core.process_event(Event::History(history::Event::Connect("local".into()))) {
    if let Effect::History(mut request) = effect {
        let result = history::engine::execute(&mut queries, "local", &request.operation);
        let effects = core.resolve(request.as_mut(), result)?;
        // Process returned effects, including Render, in the host's effect loop.
        drop(effects);
    }
}
let history = core.view().history;
# Ok(())
# }
```

The native adapter calls `ChainQueries::history`, `search`, `operation`, `contents`,
`record_variants` and `diff`. It refreshes the derived index before reading. A
browser host forwards the same portable query contract to its engine connection.
`Open` requests require a platform adapter and return an explicit error from the
query-only adapter. Client bootstraps report an unavailable history adapter until
their real connection is supplied.

| Client action | Shared behavior |
| --- | --- |
| `Connect` / `Disconnect` | Reset chain-scoped caches and cancel previous continuations. |
| `SetFilter` | Restart history and search scans with conjunctive kinds/session/author/recorder/path filters. Preserve selection within the chain. |
| `LoadMore` / `SearchMore` | Continue after the last inspected candidate, or retry a failed page. |
| `Search` / `NavigateMatch` | Search exact case-sensitive UTF-8 bytes and navigate stable evidence destinations. Whitespace is meaningful. |
| `Select` | Select a logical item separately from a chosen evidence observation. Load item context and requested evidence. |
| `ToggleDisclosure` / `LoadItem` | Expand by logical identity and load further observations for that item. |
| `LoadEvidence` | Cache exact records/content; `refresh: true` rechecks late or changed availability. |
| `Refresh` | Discard query caches, restart from the beginning and retain selection/disclosure identities. |
| `Open` | Emit a record/Original/file/diff request with full evidence identity for the host. |

Pages inspect at most 100 candidates by default. A filtered or search page can be
empty while `next_after` remains present. `exhausted` means that this scan returned
no continuation, not that all evidence exists or no conflicts exist. The adapter
uses bounded candidate scans so filters and item lookup also include historical
records via `Operation::view`; it does not silently select schema-three-only
indexes. Search retains unavailable fields and exact byte offsets. It makes no
case-folding, ranked-search or rendered-text matching claim.

Query order is operation-ID order, never chronology. Each `ItemView.key` is a full
logical item identity, while each observation has its full physical ID and exact
encoding digest. Unsupported records and chain initialization retain observation
anchors instead of invented activity identities. All `Op::parent_ids()` are
retained; logical causes, separately recorded Links and Original references remain
distinct. Author and recorder attribution are never collapsed.

`Observation.operation_json` carries the complete engine `Op` using its shared
text/binary JSON codecs. It is a transport representation. `RawRecord.bytes`
contains the original stored encoding, including every quarantined variant.
`FieldEvidence` retains exact bytes and distinguishes available empty data from
not recorded, missing, corrupt and unresolvable content. Field selectors and
content IDs use the engine's shared JSON codecs. Original content preserves
whitespace, binary bytes and source metadata; no scan preview is used as evidence.

The core reuses `StreamState` for immutable Append/Replace reconstruction, including
out-of-order predecessors and divergent branches. Block views retain identity,
position, MIME type when available, tool attempt/channel, prefix completeness,
recorded completion and head observation. File evidence includes both exact
snapshots, recorded edits and the engine's byte comparison; clients decide how to
render those bytes. Unknown, partial and conflicted data remain explicit.

The observation cache has a soft limit of 2000, with selected and expanded items
pinned. Evidence has a soft limit of 64, with pending and selected evidence pinned.
`view.cache` reports eviction; evicted item scans become reloadable. Old-context,
superseded-search and invalid response variants cannot overwrite current state.
Query continuation IDs are local to the current core. Shell protocol 3 carries
these types through generated Swift/Kotlin codecs and excludes internal completion
events from serialized client actions.

This module provides one-shot query correlation and explicit snapshot refresh.
Durable subscriptions, multi-page snapshot consistency, reconnect reconciliation,
automatic lower-ID insert/conflict notifications and presence belong to f28.
Operation-ID cursors and `refresh()` results are not durable subscription cursors.

The legacy viewer keeps coordinate/viewport adapters until f30/f31 migrate its
graph/details consumers. Its semantic selection and preview contracts already
delegate to `idle-history`; its revisioned request/reconnect adapters remain for
f28. Native document-opening adapters move with f40. The compatibility `legacy`
preview functions are deliberately lossy and are never used by the new reducer's
evidence path.
