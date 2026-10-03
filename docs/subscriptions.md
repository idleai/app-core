# Subscriptions and reconciliation

`Event::Subscriptions` owns join lifetimes, reconnect delays and connection status.
Its `Context` contains provider, workspace, contributor and logical chain identities.
Changing any component retires pending work and clears the previous scoped view.
Returning to an earlier context does not reuse request IDs. Reconnecting the same
context preserves selection, expanded items and search position.

Shell protocol **5** adds `Effect::Subscription`, `SubscriptionResponse`,
`ViewModel.subscriptions`, and atomic history reconciliation requests/results.
Regenerate Swift/Kotlin payload codecs together with the native library. The
coordination JSON protocol remains v1.

## Host effect contract

| Action | Host responsibility | Result |
| --- | --- | --- |
| `Join` | Authorize the full context and register/buffer change notifications before returning. | `Joined { connection }` |
| `Watch` | Await a queued or future change on that connection. Buffer changes between watches and while reads run. | `Changed` or `Closed` |
| `Wait` | Execute the requested retry timer. | `Elapsed` |
| `Leave` | Release only the named connection, cancel its waits and resolve outstanding watches with `Closed`. | `Left` |

Connection handles are opaque, non-secret, unique to a join, and never reused by a
later join. A late successful join is released even if its original context has
already been retired. Hosts resolve every effect through its own continuation;
an asynchronous adapter must capture the selected connection/context at dispatch,
rather than consulting a later global selection when its work completes.

After `Joined`, the core requests a buffered watch and a replacement history read.
It reports `Reconciling` until that read succeeds. A change during the read marks
it dirty and starts another read after completion. Repeated changes coalesce.
A lost connection retires its reads and watch, waits with bounded backoff, joins
again, and obtains another replacement snapshot. Stale timers, reads, errors and
join responses cannot mutate the current context.

`Changed` must include storage changes that affect accepted records or content:
lower-ID inserts, quarantined conflicts and late blobs. It is an invalidation,
not an append-only list of operations. A provider that loses notification
continuity, including queue overflow or a changed visibility scope, returns
`Closed` and requires another join. A watch must not complete repeatedly with an
empty success when nothing changed.

`Transport` errors permit retry. `Unavailable` requires a working host/provider
adapter. `Unauthorized` clears history, sessions, projections, resources and both
configuration editors, including local prompts and pending continuations, and
stops retrying. Disconnecting an active subscription also retires these scopes.
Late results cannot restore retired state or restart its requests. A temporary
transport interruption preserves the session context and its independent recovery path.
Providers still enforce access; the core's context and status do not grant
permissions.

Resource discovery also refreshes after a buffered join or `Changed` notification.
Hosts must include resource health, membership/grant changes, controller ownership
and runtime model/action changes in those notifications. Resource refreshes
coalesce while a read is pending. Connection loss retires resource continuations;
reconnect loads authorized discovery before checking outstanding action identities.
See [resources](resources.md) for expiry clocks and mutation recovery.

## Replacement reads

`history::QueryAction::Reconcile` describes the previously loaded history/search
depth, cached logical-item windows, known operation IDs and open/cached details.
The native adapter refreshes its `ChainQueries` index once, then materializes those
windows without refreshing between pages. A remote adapter must provide the same
consistent index view for the whole response. This is a replacement of loaded
windows, not an unbounded download of the entire chain.

The reducer validates every returned page and detail in a staging model before
replacing visible state. Failed reads preserve the existing view and expose a
reconciliation error. Accepted replacements rebuild `StreamState`, retract old
search hits and missing-content entries, and retain missing/conflicted selected
record anchors without presenting their old bytes as accepted content. A current
search match follows its record identity when a lower-ID insert shifts its index.
Changing the search, filter, selection or requested details during recovery
supersedes the pending replacement. Repeated snapshots do not append duplicate
items, matches or stream content.

An operation-ID page cursor is valid only within its candidate scan. Neither that
cursor nor the return value of `ChainQueries::refresh()` can resume a durable
subscription. This implementation always joins through snapshot/reconciliation.
Providers may use the published `idle_protocol::v1::RecoveryCursor` internally for
durable coordination delivery, honoring workspace, contributor and stream scope;
they must still report lost continuity and authorize a new snapshot when required.

## Transitional consumers

`idle-history::requests`, `reconciliation` and `connection` hold the portable
request tracker, ephemeral revision validation, join lifetime, retry policy and
peer-status model, re-exported by `app_core::subscriptions`. These portable
packages now live in host-tools; the client subscription reducer stays here.

The Dioxus application consumes these modules through app-core. The
host-tools `idle-peer-state` Node/WASM binding exposes the same join and connection models to
the VS Code host's portable peer coordinator. The host executes timers, native
workers and Dev Tunnels operations and formats progress; Rust owns join
generations, retry backoff and peer phases.

VS Code owns approved peers, credentials, sharing scope and transport cleanup.
Folder-owned collection sends history invalidations through the bound app-core
adapter, including writes from editor capture and peer replication. Production
session and managed subscription providers remain f43/f60 integration work.
