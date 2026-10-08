# Semantic history

`Event::History(history::Event)` drives an independent Crux history model for each
client. Effects carry a logical chain binding and a typed `history::QueryAction`.
The host resolves that binding, executes the query and resolves the request. The
reducer does no filesystem, transport, DOM, scrolling or graph-layout work.

## Indexed Activity

The full editor and compact sidebar use `history::Event::Timeline`. Their native
contract is version 2 in `idle-history::timeline`; the host advertises this
capability before either composition requests a window. The raw history queries
below remain available for operation details and other existing consumers.

The reducer owns shared exact selection, separate editor/sidebar filters and
windows, disclosure choices, Find, request cancellation and refresh. The editor
requests 200 rows and the mini requests 40. Native requests allow at most 500 rows;
the combined retained timeline data is capped at 2,000 rows and 32 MiB. Complete
documents stay behind native file, diff, Original and operation-JSON requests.

Rows carry occurrence identities independently of logical items, full record
digests and current/retained source addresses. Selecting a row with `open: true`
requests its declared native action. `Reveal` transfers an exact selection from
the mini to the editor, seeks its occurrence, and verifies the returned address.
`Move` changes selection without opening content, including across page boundaries.
Superseded open responses cannot replace the latest selection.

Manual disclosure overrides survive refresh. Live groups begin expanded and
completed groups begin folded; completion preserves groups reported as visible.
Find temporarily expands matching groups and restores manual choices when
cleared. Filter, Find and page responses are accepted only for their pending
request and declared snapshot. Failed fetches retain the readable window.

Finishing a subscription binding retires earlier reads. Compositions that
already requested the same chain remain registered for the refresh after joining,
so a sidebar mounted during startup receives its replacement window. Rows and
selection from the earlier binding are cleared before accepting fresh results.

Web-ui reports semantic anchors and whether the viewport is at the newest row.
Refresh follows new work only at that position; otherwise it preserves the
anchor and exposes a new-activity indicator. App-core forwards native lane and
relationship data without deriving topology or pixel coordinates.

## Raw records and content

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

The native adapter delegates to `idle-history-native::query` in host-tools.
That shared reader calls `ChainQueries::history`, `search`, `operation`, `contents`,
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
| `Search` / `NavigateMatch` | Search case-sensitive UTF-8 bytes and navigate to the matching operations and fields. Whitespace is meaningful. |
| `Select` | Select a logical item or observation. Load item context, stored records and field content. An observation without an item refreshes its details and resolves the item only when recorded data supplies it. |
| `ToggleDisclosure` / `LoadItem` | Expand by logical identity and load further observations for that item. |
| `LoadOperationDetails` | Cache an operation's stored records and field content; `refresh: true` rechecks late or changed availability. |
| `Refresh` | Atomically reconcile loaded windows from the beginning, retaining visible data, selection and disclosure while the read runs. |
| `Suspend` / `Reconnect` | Retire old connection reads; on reconnect, request a replacement snapshot without reusing operation-ID page positions. |
| `Open` | Emit a record/Original/file/diff request with the full operation ID and stored-record digest for the host. |

Pages inspect at most 100 candidates by default. A filtered or search page can be
empty while `next_after` remains present. `exhausted` means that this scan returned
no continuation, not that every referenced blob is available or every conflict
is resolved. The adapter
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
`FieldContent` retains exact bytes and distinguishes available empty data from
not recorded, missing, corrupt and unresolvable content. Field selectors and
content IDs use the engine's shared JSON codecs. Original content preserves
whitespace, binary bytes and source metadata. Scan previews cannot replace these
original records or content bytes.

`QueryAction::OperationDetails` returns `OperationDetails`: the stored record
variants, resolved field content and optional file comparison for one operation.
`RecordLookupStatus` reports Found, Missing or Conflicted independently of each
field's `ContentValue` availability. These names also appear in the generated
Swift/Kotlin type packages. Their binary field and variant order remain compatible
with shell protocol 4.

The core reuses `StreamState` for immutable Append/Replace reconstruction, including
out-of-order predecessors and divergent branches. Block views retain identity,
position, MIME type when available, tool attempt/channel, prefix completeness,
recorded completion and head observation. File details include both recorded
snapshots, recorded edits and the engine's byte comparison; clients decide how to
render those bytes. Unknown, partial and conflicted data remain explicit.

The observation cache has a soft limit of 2000, with selected and expanded items
pinned. The record/content cache has a soft limit of 64 operation lookups, with pending
and selected lookups pinned.
`view.cache` reports eviction; evicted item scans become reloadable. Old-context,
superseded-search and invalid response variants cannot overwrite current state.
Query continuation IDs are local to the current core. Shell protocol 5 carries
these types through generated Swift/Kotlin codecs and excludes internal completion
events from serialized client actions.

The [subscription reducer](subscriptions.md) coordinates joins, change watches and
reconnect recovery. `Reconcile` reads materialize all loaded windows against one
refreshed index, validate the whole response, then replace cached state. Lower-ID
inserts, conflict retractions and late blobs are reconciled without appending
duplicate state. The host must buffer invalidations throughout the read.
Operation-ID cursors and `refresh()` results are not durable subscription cursors.

The shared Dioxus graph and details components consume these contracts directly.
VS Code owns native document-opening adapters and resolves full record references
through its packaged history service. The old viewer, coordinate service and
projection contracts have been removed. `idle-history` in host-tools retains the shared
selection, request tracking, peer state and recorded application types needed by
the current hosts.
