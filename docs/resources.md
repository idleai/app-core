# Resources and controller status

`Event::Resources` and `ViewModel.resources` provide shared compute, provider,
model and controller state in standalone and managed workspaces. The enclosing
`ResourceContext` binds the provider connection, workspace, authenticated
contributor, logical chain and coordination mode. Hosts and providers may appear
in multiple workspaces; discovery and permissions belong to each binding.

Shell protocol **9** includes `Effect::Resource`, `ResourceResponse` and the
`Restore` event for persisted actions. Rust/WASM hosts must handle resource effects.
Regenerate Swift/Kotlin codecs together with the native library. The f20
coordination JSON protocol remains v1.

## Discovery and availability

Send `Connect(context)` to load an authorized `ResourceSnapshot`. Rust hosts can
use `ResourceSnapshot::from_protocol` to project an f20 `RecoverySnapshot`, plus
`ResourceAdapterContext` for independently authenticated runtime facts. It checks
the workspace, chain, mode, cursor audience and runtime session bindings. Routes
and credentials stay in the host adapter; UI records carry stable identities.

`ResourceRuntimeInfo::default()` reports no connected runtime capabilities.
Production adapters must not infer support from a host/provider publication.
Only the `resource-fixtures` feature exposes the explicit scripted adapter and
`demo_snapshot(mode)` for development. Fixtures do not verify installation,
serving, runtime authorization or controller failover.

Compute, provider and model views expose effective availability and allowed
actions. Model identity is `(provider_id, model_id)`. Local providers also depend
on their serving host's health. Expired health becomes `Unknown`; an offline
resource keeps its identity. Missing dependencies disable the corresponding
actions without deleting unrelated rows. Failed refreshes retain prior rows
with unknown availability and disabled actions. Authentication/authorization
failure clears the scoped rows and retires their continuations.

The adapter supplies a provider-aligned Unix clock. The host must send
`AdvanceClock(now_ms)` as health, grants and leases reach their deadlines, even
when no notification arrives. Clock ticks cannot move backward. All expiry
boundaries are exclusive. The model performs no wall-clock I/O.

## Actions and permissions

`SelectHost` and `SelectProvider` change local navigation only. `Execute` emits a
`ResourceOperationKind::Mutate` for one of these runtime actions:

| Mutation | Required current shared state | Runtime responsibility |
| --- | --- | --- |
| `ConnectHost` | Healthy host, connected adapter, explicit compute `Connect` grant. | Authenticate and authorize the connection; no implied file/process/session access. |
| `SelectModel` | Healthy published model/provider, provider `UseModels` grant, connected adapter, authorized runtime target. | Validate the exact session/host/runtime and adopt the model; recheck Control ownership when an epoch is present. |
| `InstallModel` | Healthy host advertising `LocalModels`, compute `ManageModels` grant, supported runtime catalog package and connected adapter. | Authorize, install and serve through its supported path; publish model/provider health separately. |

Ownership, workspace roles and session invitations never substitute for compute
or provider grants. Current membership and grant expiry/revocation gate all
actions. A peer can use a shared local model with a provider grant without
permission to administer its host. External-provider authentication remains
independent of managed workspace sign-in. All enabled actions are presentation
advice: the responsible runtime/coordination provider must authorize each call.

The runtime supplies model targets and installation packages. A model target
includes session, runtime and host identities, plus the controller epoch when
applicable. The selection view retains the last runtime-confirmed choice while
a new one is pending. The catalog carries supported package IDs, not arbitrary
download URLs or executable commands.

## Pending work and recovery

Before `Execute`, persist a unique `ResourceRequest` in the host with its original
first-receipt deadline. Deduplication is scoped to the workspace and authenticated
contributor. Preserve that identity, deadline and exact mutation across retries;
the host verifies attribution using its authenticated connection. Continuation
tokens and foreign-language response IDs are client-local and are not retry keys.

Return `ResourceResult::Progress` with the exact context/request, an increasing
action revision and an explicit stage. `Received` is a durable routing receipt
only. `Running`, `Succeeded` and `Failed` must be authenticated runtime facts.
Coordination commits cannot be converted into runtime success. A host combining
receipts and runtime progress must retain a monotonic action revision across
those sources, retries and reconnects.

The view retains progress separately from transport/recovery errors. `CheckStatus`
queries the original request without executing it again. `Unknown` leaves the
outcome uncertain. Even a transport error with `Never` advice cannot establish
that execution stopped; report a retained `Failed` runtime stage for a confirmed
terminal refusal or failure. `Retry` is offered only for explicit `SameRequest` advice,
after its retry time, before the original deadline and while current permissions
and capabilities still permit the operation. Pending conflicts cannot be bypassed
by sending a second request ID. There is no automatic execution retry or polling.

Runtime success triggers a fresh discovery read; completion alone cannot invent
an installed publication or a selected model. `Refresh` also queries pending
action statuses. Hosts should deliver shared subscription invalidations when
progress changes, so all clients converge on the retained runtime facts.
Notifications received during a mutation/status request remain queued. Once that
request finishes and discovery is ready, the core queries status again unless the
result is terminal. Multiple notifications coalesce; an ordinary pending response
does not start a polling loop.

`Suspend` retires continuations and marks pending operations uncertain. Same-context
`Reconnect` preserves their immutable requests, refreshes discovery and then checks
status. Switching workspace, provider, audience, chain or mode clears this client's
resource state and rejects old responses. After client destruction or a context
switch, the host sends `Connect(context)` followed by `Restore { context, request,
mutation }` for each persisted unresolved action. Restoration requires its exact
original context, identity, deadline and intent. It records an uncertain pending
action and requests its original status once authorized discovery is ready; it
never dispatches execution or restores saved success/retry claims. Restore may
arrive while discovery or a subscription join is pending. Duplicate restores are
idempotent; changing an existing request's deadline or intent is rejected.

An elapsed first-receipt deadline, removed publication or changed capability does
not prevent restoration or status lookup. Execution retries still require explicit
runtime advice, an unexpired deadline and current permissions/capabilities. Hosts
retain unresolved requests until an authenticated terminal result is recorded.
Repository navigation inside the same workspace preserves resource selection.

## Controller presentation

`ControllerView` separates assignment from runtime phase and health. Ownership
retains the authority's full 64-bit epoch watermark and optional lease. A lease
does not imply a running controller; runtime reports must match its exact holder
and epoch. Expired leases show `Expired`, expired runtime observations show an
unknown phase, and lost delivery continuity makes assignment unknown. None of
these conditions elect a successor or stop a runtime.

Replacement validation rejects decreasing epochs, changed holders under the same
epoch, conflicting resource revisions and reactivation of revoked grant records.
Controller inference and ownership enforcement remain in Evo and the coordination
provider. Live runtime verification belongs to f15/f16 and the later integration
features; this module supplies the shared client contract and fixtures.
