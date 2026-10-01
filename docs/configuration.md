# Settings and agent rules

`configuration::Configuration` owns two independent editors in the root
`ViewModel::configuration`: `settings` and `agent_rules`. Each presents its current
provider record, draft, base revision, dirty/conflict flags, validation feedback,
load state, save state, immutable pending save and available actions.

Shell protocol **10** adds `Event::Configuration`, `Effect::Configuration`,
`EffectFfi::Configuration` and `ConfigurationResponse`. Native clients must
regenerate matching codecs. Rust hosts resolve the boxed typed request with
`request.as_mut()`. The coordination envelope remains v1.

## Provider boundary

Connect with `ConfigurationContext`, containing the configured adapter name,
workspace, authenticated contributor, logical chain and `WorkspaceMode`.
Standalone and managed contexts produce the same operations through the selected
adapter. No Offstage sign-in is assumed for standalone operation.

`Connect` loads Settings and Agent Rules separately. Resolve each
`ConfigurationAction::Load` with `ConfigurationResult::Loaded`, including the
matching context/document, the complete `ConfigurationRecord`, and the provider's
current `can_edit` capability. `record: None` means the document has never existed;
it must not represent a failed, hidden or deleted read. An error leaves the last
confirmed value and draft intact and disables saving until a successful refresh.

Version 1 documents contain a full JSON object in `ConfigurationValue::json`.
`schema_version` is the document format version; `revision` is the independent,
positive provider-assigned concurrency token. All fields, including unknown
settings and rule fields, remain in the document. App-core checks JSON syntax and
object shape only. Domain schemas, defaults and rule meanings stay with their
owners. Unsupported document formats remain visible and read-only; the client
does not migrate or overwrite them. Revisions retain all 64 bits in native codecs.

## Editing and conflicts

`Edit { document, json }` replaces that editor's draft, including temporarily
invalid JSON. It never mutates the confirmed record. `validation_error` supplies
form feedback; `actions` lists the currently available intents. A dirty draft can
save only after a successful load, with supported format, provider edit capability,
valid JSON and no conflict or unresolved save.

`Refresh` and subscription invalidations reload both documents. Clean drafts track
new values. Dirty drafts keep their original text and base revision. A newer
record makes `conflict` visible and disables Save. After reviewing the current
record alongside the draft, the user can `Rebase` to keep the draft against the
new revision, or `Discard` to use the current saved content. Neither action emits
a write. Rebase is an explicit overwrite decision, not an automatic field merge.

The Settings and Agent Rules navigation destinations consume their respective
editors; loading, edits and saves in one do not overwrite the other.

## Saves and uncertain outcomes

`Save { document, request }` freezes the draft and its base revision in
`ConfigurationSave`. The host supplies a unique persisted `request_id` and its
original first-receipt deadline. The core rejects reuse across documents within
the active context. Hosts must maintain uniqueness across restarts and other
mutation types as required by the coordination contract.

The effect includes the expected revision, or an explicit create-if-absent
precondition. `ConfigurationOperation::write_request(verified_contributor)` builds
the existing `idle_protocol::v1::api::Request<ConfigurationWrite>` with
`Change<ConfigurationValue>` and `WriteCondition`. The helper rejects a different
authenticated contributor. The host selects the transport; this helper does not
specify an endpoint or bypass provider authentication.

Providers authorize every write and atomically check its precondition, persist
the complete value, assign a revision and retain the request result. Replaying
the same identity, deadline and payload returns its original outcome. Reusing an
identity for another payload must fail. Expired or unrecognized requests must
fail closed. Managed configuration and its durable change distribution belong to
Offstage (f55); standalone persistence/coordination belongs to Evo's standalone
adapter (f18). Evo remains responsible for interpreting and enforcing agent rules
at execution boundaries (f15).

Return one of these results through the matching continuation:

- `Saved { request, snapshot }`: durable commit with the exact submitted value
  and a higher revision. A routing receipt alone cannot produce this result.
- `Rejected { request, error }`: a definite, retained refusal establishing that
  this exact request did not commit. Keep the draft and show the error. Revision
  conflicts and access refusals trigger a reload before another write is offered.
- `Err(error)`: transport failure or unknown outcome. Retain the frozen request,
  show `Uncertain`, and offer `RetrySave` after a successful read/connection. The
  retry resends the unchanged original save, even if the user has edited further.
  An unknown outcome must never be converted into a definite rejection merely
  because a later lookup or authorization check failed.

Do not retry automatically. The adapter must honor any provider retry deadline
or status-query requirement before replaying a save. It may query the original
request's status and return its retained outcome through the same effect.

Users can continue editing during a save. A successful acknowledgement advances
the draft's base to the committed revision while preserving newer text. `Saved`
reports the last committed revision independently of `dirty`. Pending reads are
coalesced and delayed during writes. A retained save result cannot roll back a
newer confirmed record or restore an older editing capability.

## Connection lifecycle

Workspace/chain/mode or authenticated audience changes retire continuations and
clear the old editors. Results from a prior context cannot repopulate them, even
after returning to the same context. Repository navigation within a workspace
preserves its configuration.

With subscriptions connected, configuration loading waits for the buffered join.
Notifications refresh records; transport loss suspends loads and marks any
in-flight save uncertain while preserving drafts. Reconnect reloads both editors
before saves or retries become available. Discard and Rebase are unavailable while
a save remains unresolved.

Rust tests exercise both modes, independent editors, read failures, format checks,
concurrent edits, conflicts, capability changes, immutable retries, late results,
subscription and workspace bindings, and shell response validation. Swift/Kotlin
smoke tests exercise generated configuration codecs through the native library.
Live provider persistence, event distribution and Evo enforcement remain with
their owning features and the complete-product integration suites.
