# Shared projections

`idle_protocol::v1::projections::ProjectionSnapshot` is the shared input contract
for Evo, standalone adapters, Offstage and app-core. A version-1 snapshot names
one workspace and one logical chain and supplies each destination exactly once:
activity, task, error, triage and need-input. The schema is published separately
as [projections-v1.json](https://github.com/idleai/host-tools/blob/main/crates/idle-protocol/schemas/projections-v1.json).
Call `validate()` before consuming a decoded snapshot. Both directions of the
app-core adapter validate the complete input.

Each input carries ordered rows, an optional scope total, explicit completeness,
freshness and limitations. Complete inputs require an exact total and no known
gaps. Partial inputs explain their bounds or unresolved records/content;
unavailable inputs carry no rows or total. A controller adapter that is absent
therefore cannot appear as a successfully loaded empty board.

Row keys are stable within a destination. Titles, summaries, statuses and labels
come from the provider. App-core uses them for display and literal filtering;
it does not assign taxonomy, rank severity, infer task transitions, choose a
winning controller observation or persist annotations. f11 owns versioned
controller Note/Link payload meanings and maps them into these inputs. Durable
controller state and recorded summaries remain in Evo/EditChain; f54 can serve
rebuildable managed results using this same contract.

Source and related references carry full lowercase 256-bit observation/item
identities, plus the exact stored-record digest when supplied. When both identities
are present, the item belongs to that observation. Note targets, logical Links
and summary coverage use separate related references. Missing observations retain
their full address without a guessed item or a selected conflicting variant.
Every reference is relative to the snapshot's chain. Renderers keep these fields
for exact history drill-down instead of substituting shortened display IDs.
Every row source names an observation; a related reference may name only an item.

Freshness has an explicit current/stale/unknown assessment, optional generation
time and an opaque provider checkpoint. Generation time, query candidate order
and index refresh counts do not establish processing progress. A current partial
result is current only for its declared coverage. JSON counts and timestamps use
lossless decimal strings; shell payloads use native `u64` values.

## Client and host flow

Send `Event::Projections(projections::Event::Connect(context))` with the same
provider, authenticated contributor, workspace and chain used by subscriptions.
The root enforces active workspace and subscription bindings. This works in both
coordination modes. Repository navigation within a workspace retains projections;
workspace, provider or audience changes retire pending results and clear old rows.

The core emits `Effect::Projection(Box<Request<ProjectionQuery>>)`. The host
authorizes the context and returns a `ProjectionSnapshot`; native shells return
the equivalent `ProjectionResponse`. Convert a shared JSON input with
`projections::ProjectionSnapshot::try_from(shared_snapshot)`. Respond to each
effect through its original continuation and process all follow-up effects.

The root view exposes `projections.activity`, `tasks`, `errors`, `triage` and
`need_input`. Each has supplied rows and freshness, scope total, loaded count,
visible count and a local filter. No geometry or rendering is included.

| Event | Behavior |
| --- | --- |
| `SetFilter` | Conjunctive literal title/summary text, exact status and all requested labels. Preserves supplied totals and selection. |
| `Select` | Selects a stable row key, independent of its current observations. |
| `Inspect` | Checks that the row supplied the requested source/related reference and selects it in history. Observation-only references refresh exact details and resolve an item only when recorded data supplies it. |
| `Refresh` | Requests a complete replacement with `refresh_sources: true`; the host revalidates upstream sources and retained rows become stale. |
| `Changed` | Reconciles a background change with `refresh_sources: false`, permitting recent source reads. Changes during a read queue one follow-up; an explicit refresh takes priority. |
| `SetLimit` | Sets the engine candidate budget from 1 through 1000 (default 100) and refreshes. |
| `Suspend` / `Reconnect` | Retires pending reads, then reads a new snapshot after reconnection. |
| `Disconnect` | Clears local state without stopping a controller or changing persisted history. |

Input validation is atomic. Wrong-scope, malformed and retired results cannot
replace any destination. Failed refreshes retain old rows as stale; successful
replacement removes retracted rows and clears selection only when its key is gone.
Subscription joins start buffering notifications before projection reads. Changes
during a read coalesce into one follow-up. Completed snapshots advance the rows
but remain stale until a read finishes without another invalidation. Context,
limit and connection changes retire pending reads. Hosts also invalidate
when controller-derived state changes without a new visible history record.

## Native engine adapter and fixtures

`projections::engine::execute(queries, chain, query, mapper)` uses
`ChainQueries::history`, `relationships`, `contents` and `operation` after one
refresh. The engine receives only a caller-resolved chain; the host handles
workspace authorization. Browser hosts forward the same portable query to their
engine/provider connection. No filesystem calls run in a reducer.

The mapper receives full accepted operations, exact field bytes or explicit
availability, all recorded relationships, the candidate boundary and known gaps.
Legacy activities use `Operation::view`. Quarantined observations are excluded as
facts and reported as limitations; late blobs can resolve missing fields on the
next read. Source digests identify original encoded records. Full operations keep
author/recorder attribution, every causal parent, Link relation names, non-history
endpoints and summary coverage, including opaque frontiers.

The adapter supplies the activity input. A `ProjectionMapper` supplies the other
four inputs, using additional read-only engine queries if required. It must not
treat the bounded activity page as all controller state. The default
`UnavailableMapper` reports that the controller mapping is unavailable. Native
activity freshness remains unknown because an index refresh is not a controller
checkpoint. Reads always begin at the start, so lower-ID inserts and conflict
retractions are included; bounds remain explicit when more candidates exist.

Enable `projection-fixtures` for `projections::fixtures::seed` and `FixtureMapper`.
The seed writes validated schema-three Notes, a Link with three causal parents,
and a Message/Summary with explicit coverage through `Engine::append`. The fixture
payload namespace is `app-core/projection-fixture/v1`; it is not a production
controller contract. The default build excludes the fixture adapter.

Tests exercise all destinations, exact references, filtering/counts, context and
subscription changes, missing/late content, bounded reads, duplicate delivery,
lower-ID inserts, conflict retractions and index rebuilds. Swift and Kotlin smoke
checks exercise the generated projection codecs. Live f11/f13 mapping and f54
managed service integration remain with those feature owners.

Standalone repository hosts can supply GitHub-derived inputs through
`idle-repository`. Optional row source URLs are separate from exact history
references and must pass HTTPS validation. The native host rechecks stored source
hashes and Original content before admitting rows. The four GitHub views and their read bounds are
documented in [repository integration](repository.md); Activity remains an engine
read even if that adapter is unavailable.
