use crux_core::Request;

use crate::{Core, Effect, Event as RootEvent, workspace::WorkspaceMode};

use super::{Event, SessionAction, SessionOperation, SessionResult, SessionSnapshot, scripted};

mod adapter_cases;
mod input_cases;
mod observation_cases;
mod recovery_cases;
mod relocation_cases;
mod root_cases;
mod sharing_cases;
mod wire_cases;

fn send(core: &Core, event: Event) -> Vec<Effect> {
    core.process_event(RootEvent::Sessions(event))
}

fn select(core: &Core) {
    let _effects = send(core, Event::Select(Some("session-shared".into())));
}

fn mutation_id(id: &str) -> super::SessionMutationId {
    super::SessionMutationId {
        request_id: id.into(),
        expires_at_ms: 2000,
    }
}

fn submit(core: &Core, id: &str) -> Request<SessionOperation> {
    request(send(
        core,
        Event::Submit {
            id: mutation_id(id),
            text: "  Exact prompt\n🌍  ".into(),
        },
    ))
}

fn attribution(operation: &SessionOperation) -> super::SessionRequest {
    match &operation.action {
        SessionAction::Mutate { request, .. } => Some(request.clone()),
        SessionAction::Snapshot
        | SessionAction::Watch { .. }
        | SessionAction::RequestStatus(_)
        | SessionAction::InputStatus(_) => None,
    }
    .expect("expected mutation")
}

fn received(operation: &SessionOperation) -> SessionResult {
    SessionResult::Acknowledged(super::SessionAcknowledgement::Received(
        super::SessionReceipt {
            request: attribution(operation).key(),
            received_at_ms: 1000,
            retry_until_ms: 2500,
        },
    ))
}

fn update(
    operation: &SessionOperation,
    revision: u64,
    state: super::SessionInputState,
) -> super::SessionInputUpdate {
    let (request, session_id) = match &operation.action {
        SessionAction::Mutate {
            request,
            mutation: super::SessionMutation::Submit { session_id, .. },
        } => Some((request, session_id)),
        SessionAction::Mutate { .. }
        | SessionAction::Snapshot
        | SessionAction::Watch { .. }
        | SessionAction::RequestStatus(_)
        | SessionAction::InputStatus(_) => None,
    }
    .expect("expected input submission");
    super::SessionInputUpdate {
        input: super::SessionInputRef {
            session_id: session_id.clone(),
            request: request.key(),
        },
        contributor: request.contributor.clone(),
        runtime_id: "runtime-evo".into(),
        revision,
        state,
    }
}

fn changes(watch: &Request<SessionOperation>, changes: Vec<super::SessionChange>) -> SessionResult {
    let after = match &watch.operation.action {
        SessionAction::Watch { after } => Some(after),
        SessionAction::Mutate { .. }
        | SessionAction::Snapshot
        | SessionAction::RequestStatus(_)
        | SessionAction::InputStatus(_) => None,
    }
    .expect("expected change watch");
    let mut through = after.clone();
    let events = changes
        .into_iter()
        .map(|change| {
            through.position = through
                .position
                .checked_add(1)
                .expect("fixture event position");
            super::SessionChangeEvent {
                position: through.position,
                change,
            }
        })
        .collect();
    SessionResult::Changes(super::SessionChanges {
        after: after.clone(),
        through,
        events,
        now_ms: 1000,
    })
}

fn deliver(
    core: &Core,
    mut watch: Request<SessionOperation>,
    changes_to_deliver: Vec<super::SessionChange>,
) -> Request<SessionOperation> {
    let result = changes(&watch, changes_to_deliver);
    request(
        core.resolve(&mut watch, Ok(result))
            .expect("change response"),
    )
}

fn runtime_error() -> super::SessionError {
    super::SessionError {
        code: super::SessionErrorCode::Unavailable,
        message: "Runtime connection unavailable".into(),
        retry: super::SessionRetryAdvice::SameRequest {
            not_before_ms: None,
        },
    }
}

fn requests(effects: Vec<Effect>) -> Vec<Request<SessionOperation>> {
    effects
        .into_iter()
        .filter_map(|effect| match effect {
            Effect::Session(request) => Some(*request),
            Effect::Render(_)
            | Effect::HostInfo(_)
            | Effect::History(_)
            | Effect::Workspace(_)
            | Effect::Subscription(_)
            | Effect::Projection(_) => None,
        })
        .collect()
}

fn request(effects: Vec<Effect>) -> Request<SessionOperation> {
    let mut requests = requests(effects);
    assert_eq!(requests.len(), 1, "one session effect");
    requests.pop().expect("one session request")
}

fn snapshot(mode: WorkspaceMode, actor: &str) -> SessionSnapshot {
    scripted::demo_snapshot(mode, actor).expect("valid session fixture")
}

fn connected(snapshot: SessionSnapshot) -> (Core, Request<SessionOperation>) {
    let core = Core::new();
    let mut load = request(core.process_event(RootEvent::Sessions(Event::Connect(
        snapshot.context.clone(),
    ))));
    assert_eq!(
        load.operation.action,
        SessionAction::Snapshot,
        "connection reads authoritative state"
    );
    let effects = core
        .resolve(&mut load, Ok(SessionResult::Snapshot(Box::new(snapshot))))
        .expect("snapshot response");
    let watch = request(effects);
    (core, watch)
}

#[test]
fn owned_and_invited_sessions_have_explicit_item_bindings_in_both_modes() {
    for mode in [WorkspaceMode::Standalone, WorkspaceMode::Managed] {
        for actor in ["contributor-alice", "contributor-bob"] {
            let (core, _watch) = connected(snapshot(mode, actor));
            let view = core.view().sessions;
            assert!(!view.sessions.is_empty(), "authorized sessions are visible");
            let _effects = core.process_event(RootEvent::Sessions(Event::Select(Some(
                "session-shared".into(),
            ))));
            assert_eq!(
                core.view()
                    .sessions
                    .selected_history
                    .map(|binding| binding.item),
                Some("a".repeat(64)),
                "logical item is explicit and differs from directory identity"
            );
        }
    }
}
