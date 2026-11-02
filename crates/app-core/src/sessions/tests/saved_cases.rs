use super::{connected, deliver, request, requests, select, send, snapshot, submit, update};
use crate::{
    sessions::{
        Event, SessionAction, SessionChange, SessionInputState, SessionLoadState, SessionResult,
    },
    workspace::WorkspaceMode,
};

#[test]
fn restored_prompt_only_queries_status_and_keeps_the_original_text() {
    let mut source = snapshot(WorkspaceMode::Standalone, "contributor-bob");
    let (first, _watch) = connected(source.clone());
    select(&first);
    let submitted = submit(&first, "saved-before-send");
    let fact = update(
        &submitted.operation,
        1,
        SessionInputState::Accepted { at_ms: 1100 },
    );
    let mut retained = fact.clone();
    retained.text = Some("  Exact prompt\n🌍  ".into());
    source.inputs.push(retained);
    let (reopened, _watch) = connected(source);
    select(&reopened);
    let mut lookup = request(send(
        &reopened,
        Event::Restore(Box::new(submitted.operation.clone())),
    ));
    assert!(matches!(
        lookup.operation.action,
        SessionAction::RequestStatus(_)
    ));
    assert_eq!(
        reopened
            .view()
            .sessions
            .prompts
            .first()
            .expect("retained prompt")
            .text
            .as_deref(),
        Some("  Exact prompt\n🌍  ")
    );
    let effects = reopened
        .resolve(&mut lookup, Ok(SessionResult::Input(fact)))
        .expect("retained status");
    assert!(
        requests(effects)
            .iter()
            .all(|request| !matches!(request.operation.action, SessionAction::Mutate { .. }))
    );
    assert_eq!(reopened.view().sessions.prompts.len(), 1);
    assert!(matches!(
        reopened
            .view()
            .sessions
            .mutations
            .first()
            .expect("restored mutation")
            .state,
        crate::sessions::SessionMutationState::RuntimeReported
    ));
    assert!(
        reopened
            .view()
            .sessions
            .prompts
            .first()
            .expect("retained prompt")
            .runtime
            .is_some()
    );
    let effects = send(&reopened, Event::Restore(Box::new(submitted.operation)));
    assert!(
        requests(effects)
            .iter()
            .all(|request| !matches!(request.operation.action, SessionAction::Mutate { .. }))
    );
    assert_eq!(reopened.view().sessions.prompts.len(), 1);
}

#[test]
fn saved_requests_from_another_contributor_cannot_enter_the_new_session() {
    let (first, _watch) = connected(snapshot(WorkspaceMode::Standalone, "contributor-bob"));
    select(&first);
    let saved = submit(&first, "foreign-contributor").operation;
    let (other, _watch) = connected(snapshot(WorkspaceMode::Standalone, "contributor-alice"));
    let effects = send(&other, Event::Restore(Box::new(saved)));
    assert!(requests(effects).is_empty());
    assert!(other.view().sessions.prompts.is_empty());
    assert!(other.view().sessions.action_error.is_some());
}

#[test]
fn live_session_can_gain_recorded_history_once_without_fabricating_a_binding() {
    let mut source = snapshot(WorkspaceMode::Standalone, "contributor-bob");
    let session = source
        .sessions
        .iter_mut()
        .find(|session| session.id == "session-shared")
        .expect("shared");
    let binding = session.history.take().expect("recorded binding");
    let mut bound = session.clone();
    bound.revision += 1;
    bound.history = Some(binding.clone());
    let (core, watch) = connected(source);
    select(&core);
    assert!(core.view().sessions.selected_history.is_none());
    let watch = deliver(&core, watch, vec![SessionChange::Session(bound.clone())]);
    assert_eq!(core.view().sessions.selected_history, Some(binding));
    bound.revision += 1;
    bound.history = None;
    let result = super::changes(&watch, vec![SessionChange::Session(bound)]);
    let mut watch = watch;
    let _effects = core
        .resolve(&mut watch, Ok(result))
        .expect("invalid later binding");
    assert!(matches!(
        core.view().sessions.updates,
        SessionLoadState::Failed(_)
    ));
}

#[test]
fn snapshot_text_enriches_an_input_without_changing_its_runtime_revision() {
    let (author, _watch) = connected(snapshot(WorkspaceMode::Standalone, "contributor-bob"));
    select(&author);
    let submitted = submit(&author, "retained-text");
    let fact = update(
        &submitted.operation,
        1,
        SessionInputState::Accepted { at_ms: 1100 },
    );
    let mut source = snapshot(WorkspaceMode::Standalone, "contributor-alice");
    source.inputs.push(fact);
    let (reader, _watch) = connected(source.clone());
    assert_eq!(
        reader
            .view()
            .sessions
            .prompts
            .first()
            .expect("retained prompt")
            .text,
        None
    );
    source.inputs.first_mut().expect("retained input").text = Some("Exact shared prompt".into());
    let mut refresh = request(send(&reader, Event::Refresh));
    let _effects = reader
        .resolve(
            &mut refresh,
            Ok(SessionResult::Snapshot(Box::new(source.clone()))),
        )
        .expect("enriched input");
    assert_eq!(
        reader
            .view()
            .sessions
            .prompts
            .first()
            .expect("retained prompt")
            .text
            .as_deref(),
        Some("Exact shared prompt")
    );
    source.inputs.first_mut().expect("retained input").text = Some("Conflicting prompt".into());
    let mut refresh = request(send(&reader, Event::Refresh));
    let _effects = reader
        .resolve(&mut refresh, Ok(SessionResult::Snapshot(Box::new(source))))
        .expect("conflicting input");
    assert!(matches!(
        reader.view().sessions.load,
        SessionLoadState::Failed(_)
    ));
    assert_eq!(
        reader
            .view()
            .sessions
            .prompts
            .first()
            .expect("retained prompt")
            .text
            .as_deref(),
        Some("Exact shared prompt")
    );
}
