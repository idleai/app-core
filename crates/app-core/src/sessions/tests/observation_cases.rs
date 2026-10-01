use super::{
    connected, deliver, request, requests, runtime_error, select, send, snapshot, submit, update,
};
use crate::{
    sessions::{
        Event, SessionChange, SessionErrorCode, SessionGrantStatus, SessionInputState,
        SessionMutationState, SessionPermission, SessionResult, SessionSnapshot,
    },
    workspace::WorkspaceMode,
};

fn observation_snapshot() -> SessionSnapshot {
    let mut source = snapshot(WorkspaceMode::Managed, "contributor-bob");
    let grant = source
        .grants
        .iter_mut()
        .find(|grant| grant.grantee == "contributor-bob")
        .expect("Bob grant");
    let mut input_grant = grant.clone();
    input_grant.id = "submit-only".into();
    input_grant.permissions = vec![SessionPermission::SubmitInput];
    input_grant.expires_at_ms = None;
    grant.permissions = vec![SessionPermission::Observe];
    grant.expires_at_ms = Some(1200);
    source.grants.push(input_grant);
    let (alice, _watch) = connected(snapshot(WorkspaceMode::Managed, "contributor-alice"));
    select(&alice);
    let submission = submit(&alice, "alice-input");
    source.inputs = vec![update(
        &submission.operation,
        1,
        SessionInputState::Accepted { at_ms: 1100 },
    )];
    source
}

#[test]
fn observe_expiry_hides_runtime_facts_retires_reads_and_preserves_local_submission() {
    let mut source = observation_snapshot();
    let remote = source.inputs.first().expect("Alice input").clone();
    let (core, watch) = connected(source.clone());
    select(&core);
    let mut lookup = request(send(&core, Event::RefreshInput(remote.input.clone())));
    let mut local = submit(&core, "bob-input");
    let local_fact = update(
        &local.operation,
        1,
        SessionInputState::Accepted { at_ms: 1100 },
    );
    let _effects = core
        .resolve(&mut local, Ok(SessionResult::Input(local_fact.clone())))
        .expect("local input reply");
    assert_eq!(core.view().sessions.prompts.len(), 2, "Observe is active");

    let _effects = send(&core, Event::Tick(1200));
    let view = core.view().sessions;
    assert!(
        view.sessions.first().is_some_and(|session| {
            session.actions.contains(&SessionPermission::SubmitInput)
                && !session.actions.contains(&SessionPermission::Observe)
        }),
        "submission permission remains independent"
    );
    assert_eq!(view.prompts.len(), 1, "remote runtime history is hidden");
    let prompt = view.prompts.first().expect("local prompt");
    assert_eq!(prompt.text.as_deref(), Some("  Exact prompt\n🌍  "));
    assert_eq!(prompt.contributor, local_fact.contributor);
    assert!(prompt.runtime.is_none(), "runtime details require Observe");
    assert_eq!(
        prompt.submission,
        Some(SessionMutationState::RuntimeReported)
    );

    for input in [remote.input.clone(), local_fact.input] {
        assert!(
            requests(send(&core, Event::RefreshInput(input))).is_empty(),
            "all input status reads require Observe"
        );
    }
    let before = core.view();
    let _effects = core
        .resolve(&mut lookup, Ok(SessionResult::Input(remote.clone())))
        .expect("late observation reply");
    assert_eq!(core.view(), before, "retired reads cannot change state");
    let _watch = deliver(&core, watch, vec![SessionChange::Input(remote)]);
    assert_eq!(
        core.view().sessions.prompts.len(),
        1,
        "watch cannot restore remote data"
    );

    source.cursor.position = 100;
    let mut refresh = request(send(&core, Event::Refresh));
    let _watch = request(
        core.resolve(&mut refresh, Ok(SessionResult::Snapshot(Box::new(source))))
            .expect("refresh after expiry"),
    );
    let _effects = send(&core, Event::Tick(1100));
    assert_eq!(
        core.view().sessions.prompts,
        view.prompts,
        "refresh and clock regression preserve expiry"
    );
    let mut next = submit(&core, "bob-after-expiry");
    let accepted = update(
        &next.operation,
        1,
        SessionInputState::Accepted { at_ms: 1250 },
    );
    let _effects = core
        .resolve(&mut next, Ok(SessionResult::Input(accepted)))
        .expect("submission without observation");
    assert_eq!(
        core.view().sessions.prompts.len(),
        2,
        "submission is still available"
    );
    assert!(
        core.view()
            .sessions
            .prompts
            .iter()
            .all(|prompt| prompt.text.is_some() && prompt.runtime.is_none()),
        "local text survives without exposing runtime data"
    );
}

#[test]
fn observe_revocation_retires_queries_even_when_an_input_grant_remains() {
    let source = observation_snapshot();
    let remote = source.inputs.first().expect("Alice input").clone();
    let mut grant = source
        .grants
        .iter()
        .find(|grant| {
            grant.grantee == "contributor-bob" && grant.permissions == [SessionPermission::Observe]
        })
        .expect("observation grant")
        .clone();
    grant.revision = 2;
    grant.status = SessionGrantStatus::Revoked {
        at_ms: 1100,
        by: "contributor-alice".into(),
    };
    let (core, watch) = connected(source);
    select(&core);
    let mut lookup = request(send(&core, Event::RefreshInput(remote.input.clone())));
    let _watch = deliver(
        &core,
        watch,
        vec![
            SessionChange::Grant(grant),
            SessionChange::Input(remote.clone()),
        ],
    );
    assert!(
        core.view().sessions.prompts.is_empty(),
        "revocation hides remote data"
    );
    let before = core.view();
    let _effects = core
        .resolve(&mut lookup, Ok(SessionResult::Input(remote)))
        .expect("late revoked lookup");
    assert_eq!(
        core.view(),
        before,
        "lookup is retired independently of SubmitInput"
    );
    let _submission = submit(&core, "bob-after-revocation");
    assert_eq!(
        core.view().sessions.prompts.len(),
        1,
        "revocation retains submission access"
    );
}

#[test]
fn connecting_without_observe_does_not_publish_remote_history() {
    let mut source = observation_snapshot();
    source.now_ms = 1200;
    let remote = source.inputs.first().expect("Alice input").clone();
    let (core, watch) = connected(source);
    assert!(
        core.view().sessions.prompts.is_empty(),
        "snapshot requires observation access"
    );
    let _watch = deliver(&core, watch, vec![SessionChange::Input(remote)]);
    assert!(
        core.view().sessions.prompts.is_empty(),
        "watch requires observation access"
    );
    select(&core);
    let _submission = submit(&core, "bob-without-observation");
    assert_eq!(
        core.view().sessions.prompts.len(),
        1,
        "local input remains available"
    );
}

#[test]
fn hiding_local_runtime_facts_does_not_allow_resubmission_of_accepted_input() {
    let (core, watch) = connected(observation_snapshot());
    select(&core);
    let mut submission = submit(&core, "accepted-input");
    let accepted = update(
        &submission.operation,
        1,
        SessionInputState::Accepted { at_ms: 1100 },
    );
    let _watch = deliver(&core, watch, vec![SessionChange::Input(accepted)]);
    let _effects = core
        .resolve(&mut submission, Err(runtime_error()))
        .expect("late transport failure");
    let _effects = send(&core, Event::Tick(1200));
    let view = core.view().sessions;
    assert_eq!(view.prompts.len(), 1, "only local text remains");
    assert!(
        view.prompts
            .first()
            .is_some_and(|prompt| prompt.runtime.is_none())
    );
    assert!(
        requests(send(&core, Event::Retry("accepted-input".into()))).is_empty(),
        "hidden runtime acceptance still prevents resubmission"
    );
    assert_eq!(
        core.view().sessions.action_error.map(|error| error.code),
        Some(SessionErrorCode::Conflict)
    );
}
