# Workspace navigation and coordination adapters

`Event::Workspace(workspace::Event)` drives workspace/repository selection,
member views, peer activity and navigation for standalone and managed clients.
The reducer performs no I/O. Hosts execute boxed `Effect::Workspace` requests
and resolve them with `WorkspaceOutput`; native shells encode `WorkspaceResponse`.
Shell protocol **4** adds these operations and `ViewModel.workspace`.

## Host flow

1. Send `Workspace(Load)` to discover or refresh the authorized workspace directory.
   `WorkspaceOperation::List` uses the host's configured connections: local/peer
   configuration in standalone mode and authorized managed discovery in workspace
   mode. Starting the bootstrap handshake does not require workspace discovery or
   managed sign-in. An empty successful list is distinct from an unavailable provider.
2. Return `WorkspaceResult::Directory(Vec<WorkspaceInfo>)`. Each row includes the
   authority's workspace metadata revision, mode, chain reference and attached
   repositories. A directory can contain both modes. The user explicitly selects
   a workspace with `SelectWorkspace(id)`.
3. Selection emits `WorkspaceOperation::Snapshot { workspace_id, mode }` and a
   history query scoped only to the logical chain. Hosts authorize each operation
   through its responsible adapter. The engine never receives workspace metadata,
   membership, repository attachments, credentials or a coordination mode.
4. Return `WorkspaceResult::Snapshot(WorkspaceSnapshot)` with members and visible
   host/provider binding IDs. The reducer then requests `Presence` through the
   selected provider. Unsupported peer activity is an explicit `WorkspaceError`,
   not an empty successful observation.
5. Return `WorkspaceResult::Presence(PresenceSnapshot)` and send `Tick(unix_ms)`
   as the host clock advances. Send `RefreshWorkspace` or `RefreshPresence` for
   updates and retries. Host notification/recovery adapters can trigger these
   reads after reconciling their authoritative data.

Rust hosts can project existing `idle_protocol::v1` values directly:

```rust
use app_core::workspace::{WorkspaceInfo, WorkspaceSnapshot};
use idle_protocol::v1::{Record, events::RecoverySnapshot, workspace::Workspace};

fn directory_row(record: &Record<Workspace>) -> WorkspaceInfo {
    WorkspaceInfo::from(record)
}

fn selected_snapshot(
    snapshot: &RecoverySnapshot,
) -> Result<WorkspaceSnapshot, app_core::workspace::WorkspaceError> {
    WorkspaceSnapshot::try_from(snapshot)
}
```

The projection preserves opaque identities and full-width revisions and checks
the recovery cursor's workspace scope. The adapter remains responsible for
validating its authenticated audience and maintaining its recovery cursor.
`WorkspaceError::from(ApiError)` preserves read-error categories and the safe
provider message. All retries here are explicit read requests, not mutations.

Directory discovery and peer activity are host adapter operations, not new
endpoints in the versioned coordination protocol. Peer activity has a typed
app-core projection because f20 does not yet publish a matching payload.
Local/Evo and Offstage adapters supply it; fixtures exist only in tests and native
smoke programs.
Transport subscriptions, durable event recovery and mutation workflows remain
with their owning features. No authorization or execution grants are inferred
from these presentation models.

## Bindings and selection

Every workspace has exactly one nonempty logical chain reference. The reducer
rejects reassignment, conflicting metadata at one revision, revision regression,
and reuse of a chain by another workspace. Accepted bindings survive directory
removal and `Disconnect` for the lifetime of this client. Providers must enforce
the same invariants durably across clients.

`repository_bindings` exposes explicit `(workspace_id, repository_id, chain)`
triples. `repository_binding` is the currently selected triple. Repositories are
never globally mapped to one workspace or chain. The same repository can be
attached to two workspaces with distinct logical chains. Host and provider IDs
are likewise scoped by their enclosing `WorkspaceSnapshot`; removing a binding
does not delete a resource from other workspaces.

Standalone metadata must contain exactly one repository, which is automatically
selected. A managed workspace can contain zero, one or many repositories. One
repository is selected initially when it is the only choice; managed clients may
select `None` to view the whole workspace. A detached selected repository is
cleared. Repository changes within the same workspace preserve the history chain
and its interaction state.

Adopting managed coordination uses the same workspace, chain and repository
identities with a newer workspace revision. Updated directory metadata reloads
the selected snapshot through its new route, invalidating old provider results
while retaining navigation and the chain. Workspace switches reset the navigation
destination and isolate member, peer activity and history state. Unknown
selections report `selection_error` and keep the valid current selection.

`Navigate` selects Workspace, Members, Sessions, Projections, ComputeHosts,
ModelProviders, Activity, Settings or AgentRules. This is shared semantic
navigation; layout and section-specific domain state belong to their consumers.

Once workspace navigation is loaded, the root app owns history's chain selection.
Direct `History(Connect)` to a different chain and `History(Disconnect)` are
ignored; use workspace selection or `Workspace(Disconnect)`. Independent history
clients that never opt into workspace navigation keep the direct history API.

## Loading, freshness and failures

The view exposes independent `directory_state`, `snapshot_state` and
`presence_state`: Idle, Loading, Ready or Failed with a typed error. Cached
directory/member metadata remains visible during refresh or temporary failure;
its state identifies it as stale. Peer activity is cleared when refreshing
workspace metadata so pending reports cannot restore removed members or attachments.

`Unavailable` retains cached metadata for retry. `Unsupported` reports a missing
adapter capability. `InvalidData` reports malformed or inconsistent provider data.
`Unauthenticated`, `Forbidden` and `NotFound` represent loss of workspace access:
the selected scope and history are cleared, and pre-failure requests cannot
restore it. Use `Unsupported` when peer activity alone is unavailable; use
`Forbidden` when access to the workspace itself has been denied/revoked. Refresh
the directory after recovering access. Enforcement remains with the provider and
runtime.

Each request has a monotonic client-local continuation token. Responses are
checked against their operation and workspace scope, and obsolete continuations
are discarded even for A → B → A navigation. Disconnect invalidates every pending
workspace result. Directory replacements validate atomically before any accepted
bindings change. Metadata revisions are neither timestamps nor recovery cursors.

Members retain contributor IDs and roles independently of host IDs. Missing
contributor display metadata falls back to the contributor ID. Revoked membership
suppresses all peer activity. Each activity report retains a separate connection
ID so one person can work on several hosts or branches. Repository/file/branch
and host locations must refer to bindings in the selected snapshot.

Adapters normalize activity observation times, freshness deadlines and `as_of_ms`
to Unix milliseconds compatible with the host's `Tick` clock. Expiry is exclusive;
older clock ticks cannot resurrect expired connections. Fresh Online takes
precedence over Away, then an explicitly observed Offline. Missing, expired or
failed activity reports yield Unknown status. Offline/unknown observations expose no current
locations. A successful empty peer activity snapshot removes prior observations.

## Verification

Crux tests cover both modes, reusable resources, mode adoption, detach/removal,
out-of-order responses, invalid bindings, loading/retry, revocation, multi-device
peer activity and expiry. Shell tests cover malformed responses, result correlation,
internal-event rejection and workspace-to-history wiring. Swift and Kotlin/JVM
smoke programs exercise both modes through generated codecs, including full-width
revisions, members, peer activity expiry, navigation and typed failures.

Run `./scripts/lint.sh` and `./scripts/check.sh`. Regenerate host payload bindings
for shell protocol 5; coordination JSON protocol v1 is unchanged.

## Peer activity views and join options

`app_core::peer_activity` derives file peers and join choices from accepted workspace
and directory state. `PeerAwareness` tracks branch transitions, pending invitations
and acknowledgements across refreshes. Reset it when the selected account,
provider or recovery stream changes. `prepare_join` rechecks the selected intent
against current state; its result still requires authorization at the responsible
authority/runtime before connecting.

Hosts supply editor context and render the resulting view. The VS Code host
owns CodeLens, status items, prompts and command execution. Shared fixture JSON
lives under `crates/app-core/tests/fixtures` and is also consumed by the extension
host tests.
