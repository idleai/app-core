# Sessions

`Sessions` handles owned/invited session creation, selection, sharing and attributed
prompts through `Event::Sessions`, `Effect::Session` and `ViewModel.sessions`.
See the typed [events](../crates/app-core/src/sessions/reducer.rs),
[operations](../crates/app-core/src/sessions/operations.rs) and
[views](../crates/app-core/src/sessions/views.rs) for the API.

## Core rules

- Each `SessionInfo` binds a coordination session (`id`) to its current runtime
  (`runtime`) and an explicit EditChain chain/logical item (`history`). Hosts supply
  the full item ID; relocation preserves it. `selected_history` exposes the binding.
- Ownership and contributor identity are separate. Prompts use the authenticated
  contributor; even owners need a `SubmitInput` grant. Session grants provide no
  compute access. Revocation, expiry and workspace membership govern access.
- Pending prompts retain exact local text and attribution. Remote prompt text may
  be unavailable. Only runtime events establish acceptance, delivery order and
  execution; backend receipts, list positions and coordination cursors cannot.
- Retries preserve the original request key, payload and deadline and require
  `SameRequest` advice. `Recover` queries an uncertain result without resubmitting.
  Same-context refresh preserves selection, local text and newer runtime facts.

## Host flow

1. Send `SessionEvent::Connect(context)` with the provider, authenticated audience,
   workspace and logical chain. Resolve `Snapshot`, then each `Watch`, using
   `SessionSnapshot::from_protocol` and `SessionResult::from_recovery` with explicit
   item mappings. Once caught up, wait for updates; resolve cancelled watches.
2. Handle `Create`, `Select`, `Invite`, `Revoke` and `Submit` intents. Selection is
   local; mutations emit typed operations. Use `coordination_command` for f20
   input/grant commands or `runtime_submission` for direct runtime input.
3. For creation, allocate the runner and logical item, persist their mapping, then
   register the directory with `registration_request`. Resolve `Created` only after
   all steps succeed. Keep allocated IDs across retries; a directory commit alone
   cannot confirm runtime creation.
4. Rust hosts return results with `core.resolve(request.as_mut(), output)`; native
   hosts use `SessionResponse` with shell protocol **6**. See [runtime integration](runtime.md).
   Hosts authorize every operation and report capabilities unavailable until connected.

## Fixtures and remaining work

The optional `session-fixtures` feature exposes
`sessions::scripted::{demo_snapshot, ScriptedSessions, SessionScriptStep}` for
standalone and managed tests. Scripts pair exact operations with responses;
production hosts use connected adapters.

Live verification remains with **f14/shared-input**: concurrent contributors,
retry deduplication, consistent runtime order, reconnect during execution and
access changes. Runtime/history adapters belong to f10/f17/f18; production client
connections belong to f43/f60. Scripted responses do not verify those integrations.
