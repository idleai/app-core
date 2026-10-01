use super::{
    attribution, changes, connected, deliver, mutation_id, received, request, runtime_error,
    select, send, snapshot, submit, update,
};
use crate::{
    sessions::{
        Event, SessionAcknowledgement, SessionAction, SessionChange, SessionCompletion,
        SessionDelivery, SessionErrorCode, SessionInputState, SessionLoadState,
        SessionMutationState, SessionResult, SessionRetryAdvice,
        scripted::{ScriptedSessions, SessionScriptStep},
    },
    workspace::WorkspaceMode,
};

fn delivery(order: u64) -> SessionDelivery {
    SessionDelivery {
        accepted_at_ms: 1100,
        order,
        ordered_at_ms: 1150,
    }
}

#[test]
fn attributed_pending_prompt_receipt_and_scripted_runtime_lifecycle_are_distinct() {
    for mode in [WorkspaceMode::Standalone, WorkspaceMode::Managed] {
        let (core, mut watch) = connected(snapshot(mode, "contributor-bob"));
        select(&core);
        let mut submission = submit(&core, "input-42");
        let pending = core
            .view()
            .sessions
            .prompts
            .into_iter()
            .next()
            .expect("pending prompt");
        assert_eq!(
            pending.contributor.contributor_id, "contributor-bob",
            "invited author is not the owner Alice"
        );
        assert_eq!(
            pending.text.as_deref(),
            Some("  Exact prompt\n🌍  "),
            "text is retained exactly"
        );
        assert_eq!(
            pending.submission,
            Some(SessionMutationState::Pending),
            "client is waiting for transport"
        );
        assert!(
            pending.runtime.is_none(),
            "local pending input has no runtime facts"
        );
        let receipt = received(&submission.operation);
        let mut script = ScriptedSessions::new([SessionScriptStep {
            operation: submission.operation.clone(),
            result: Ok(receipt),
        }]);
        let output = script.execute(&submission.operation);
        let _effects = core
            .resolve(&mut submission, output)
            .expect("receipt response");
        assert_eq!(
            script.remaining(),
            0,
            "script is consumed through the production effect interface"
        );
        let prompt = core
            .view()
            .sessions
            .prompts
            .into_iter()
            .next()
            .expect("received prompt");
        assert!(
            matches!(
                prompt.submission,
                Some(SessionMutationState::Acknowledged(
                    SessionAcknowledgement::Received(_)
                ))
            ),
            "backend receipt is explicit"
        );
        assert!(
            prompt.runtime.is_none(),
            "backend receipt cannot assign order or execution"
        );
        for (revision, state) in [
            (1, SessionInputState::Accepted { at_ms: 1100 }),
            (
                2,
                SessionInputState::Ordered(delivery(9_007_199_254_740_993)),
            ),
            (
                3,
                SessionInputState::Running {
                    delivery: delivery(9_007_199_254_740_993),
                    started_at_ms: 1200,
                },
            ),
            (
                4,
                SessionInputState::Completed {
                    delivery: delivery(9_007_199_254_740_993),
                    completed_at_ms: 1500,
                    outcome: SessionCompletion::Succeeded,
                },
            ),
        ] {
            let fact = update(&submission.operation, revision, state);
            watch = deliver(&core, watch, vec![SessionChange::Input(fact.clone())]);
            assert_eq!(
                core.view()
                    .sessions
                    .prompts
                    .first()
                    .and_then(|prompt| prompt.runtime.as_ref()),
                Some(&fact),
                "only runtime facts advance the lifecycle"
            );
        }
    }
}

#[test]
fn concurrent_contributors_recover_the_same_runtime_order_without_authorship_substitution() {
    let (alice, alice_watch) = connected(snapshot(WorkspaceMode::Managed, "contributor-alice"));
    let (bob, bob_watch) = connected(snapshot(WorkspaceMode::Managed, "contributor-bob"));
    select(&alice);
    select(&bob);
    let alice_submit = submit(&alice, "same-client-id");
    let bob_submit = submit(&bob, "same-client-id");
    let alice_fact = update(
        &alice_submit.operation,
        2,
        SessionInputState::Ordered(delivery(2)),
    );
    let bob_fact = update(
        &bob_submit.operation,
        2,
        SessionInputState::Ordered(delivery(1)),
    );
    let _alice_watch = deliver(
        &alice,
        alice_watch,
        vec![
            SessionChange::Input(alice_fact.clone()),
            SessionChange::Input(bob_fact.clone()),
        ],
    );
    let _bob_watch = deliver(
        &bob,
        bob_watch,
        vec![
            SessionChange::Input(bob_fact),
            SessionChange::Input(alice_fact),
        ],
    );
    for core in [&alice, &bob] {
        let view = core.view().sessions;
        assert_eq!(
            view.prompts.len(),
            2,
            "one input per full contributor/request key"
        );
        for prompt in view.prompts {
            let expected = if prompt.contributor.contributor_id == "contributor-bob" {
                1
            } else {
                2
            };
            assert_eq!(
                prompt
                    .runtime
                    .and_then(|fact| fact.state.delivery().map(|value| value.order)),
                Some(expected),
                "arrival order is not delivery order"
            );
            assert_eq!(
                prompt.text.is_some(),
                prompt.contributor.contributor_id
                    == view.context.as_ref().expect("context").contributor_id,
                "remote text remains unknown in f20 recovery"
            );
        }
    }
}

#[test]
fn duplicates_and_old_input_revisions_are_ignored_but_conflicts_fail_atomically() {
    let (core, watch) = connected(snapshot(WorkspaceMode::Managed, "contributor-bob"));
    select(&core);
    let submission = submit(&core, "prompt");
    let completed = update(
        &submission.operation,
        4,
        SessionInputState::Completed {
            delivery: delivery(1),
            completed_at_ms: 1500,
            outcome: SessionCompletion::Cancelled,
        },
    );
    let watch = deliver(&core, watch, vec![SessionChange::Input(completed.clone())]);
    let old = update(
        &submission.operation,
        1,
        SessionInputState::Accepted { at_ms: 1100 },
    );
    let mut watch = deliver(
        &core,
        watch,
        vec![
            SessionChange::Input(completed.clone()),
            SessionChange::Input(old),
        ],
    );
    assert_eq!(
        core.view().sessions.prompts.len(),
        1,
        "replay does not duplicate input"
    );
    let mut conflicting = completed.clone();
    conflicting.revision = 5;
    conflicting.state = SessionInputState::Running {
        delivery: delivery(1),
        started_at_ms: 1200,
    };
    let result = changes(&watch, vec![SessionChange::Input(conflicting)]);
    let _effects = core
        .resolve(&mut watch, Ok(result))
        .expect("invalid runtime response");
    assert!(
        matches!(core.view().sessions.updates, SessionLoadState::Failed(_)),
        "terminal regression is a protocol error"
    );
    assert_eq!(
        core.view()
            .sessions
            .prompts
            .first()
            .and_then(|prompt| prompt.runtime.as_ref()),
        Some(&completed),
        "previous terminal result survives invalid updates"
    );
}

#[test]
fn invalid_runtime_scope_author_identity_order_and_revision_never_enter_the_view() {
    for case in 0..6 {
        let (core, watch) = connected(snapshot(WorkspaceMode::Managed, "contributor-bob"));
        select(&core);
        let submission = submit(&core, "prompt");
        let first = update(
            &submission.operation,
            2,
            SessionInputState::Ordered(delivery(1)),
        );
        let mut watch = deliver(&core, watch, vec![SessionChange::Input(first.clone())]);
        let mut bad = update(
            &submission.operation,
            3,
            SessionInputState::Running {
                delivery: delivery(1),
                started_at_ms: 1200,
            },
        );
        match case {
            0 => bad.input.request.workspace_id = "other-workspace".into(),
            1 => bad.contributor.contributor_id = "contributor-alice".into(),
            2 => bad.contributor.subject = "someone-else".into(),
            3 => bad.runtime_id = "unbound-runtime".into(),
            4 => bad.state = SessionInputState::Ordered(delivery(2)),
            _ => bad.revision = 2,
        }
        let result = changes(&watch, vec![SessionChange::Input(bad)]);
        let _effects = core
            .resolve(&mut watch, Ok(result))
            .expect("malformed runtime update");
        assert!(
            matches!(core.view().sessions.updates, SessionLoadState::Failed(_)),
            "case {case} rejects invalid runtime facts"
        );
        assert_eq!(
            core.view()
                .sessions
                .prompts
                .first()
                .and_then(|prompt| prompt.runtime.as_ref()),
            Some(&first),
            "invalid updates cannot alter confirmed order"
        );
    }
}

#[test]
fn retries_preserve_request_identity_attribution_deadline_and_exact_text() {
    let (core, _watch) = connected(snapshot(WorkspaceMode::Standalone, "contributor-bob"));
    select(&core);
    let mut submission = submit(&core, "retry");
    assert!(
        send(
            &core,
            Event::Submit {
                id: mutation_id("retry"),
                text: "  Exact prompt\n🌍  ".into()
            }
        )
        .is_empty(),
        "duplicate intent cannot duplicate in-flight work"
    );
    let _effects = send(
        &core,
        Event::Submit {
            id: mutation_id("retry"),
            text: "changed text".into(),
        },
    );
    assert_eq!(
        core.view().sessions.action_error.map(|error| error.code),
        Some(SessionErrorCode::IdempotencyConflict),
        "changed payload cannot reuse a retry key"
    );
    let original = submission.operation.clone();
    let _effects = core
        .resolve(&mut submission, Err(runtime_error()))
        .expect("temporary dispatch failure");
    let retry = request(send(&core, Event::Retry("retry".into())));
    assert_eq!(
        retry.operation, original,
        "all semantic request fields are unchanged"
    );
    assert_eq!(
        core.view().sessions.prompts.len(),
        1,
        "retry keeps one pending prompt"
    );
    assert_eq!(
        attribution(&retry.operation).contributor.contributor_id,
        "contributor-bob",
        "retry still belongs to the invited author"
    );
}

#[test]
fn uncertain_receipt_requires_status_lookup_and_unknown_does_not_trigger_resubmission() {
    let (core, _watch) = connected(snapshot(WorkspaceMode::Managed, "contributor-bob"));
    select(&core);
    let mut submission = submit(&core, "uncertain");
    let mut failure = runtime_error();
    failure.retry = SessionRetryAdvice::QueryStatus;
    let _effects = core
        .resolve(&mut submission, Err(failure))
        .expect("uncertain result");
    assert!(
        super::requests(send(&core, Event::Retry("uncertain".into()))).is_empty(),
        "uncertainty cannot be converted to a new dispatch"
    );
    let mut lookup = request(send(&core, Event::Recover("uncertain".into())));
    assert_eq!(
        lookup.operation.action,
        SessionAction::RequestStatus(attribution(&submission.operation).key()),
        "lookup uses the original contributor/request key"
    );
    let effects = core
        .resolve(&mut lookup, Ok(SessionResult::Unknown))
        .expect("unknown receipt");
    assert!(
        super::requests(effects).is_empty(),
        "unknown never authorizes automatic resubmission"
    );
    assert_eq!(
        core.view()
            .sessions
            .mutations
            .first()
            .map(|mutation| &mutation.state),
        Some(&SessionMutationState::Unknown),
        "uncertain outcome remains explicit"
    );
}

#[test]
fn direct_runtime_response_and_late_receipt_do_not_regress_execution() {
    let (core, watch) = connected(snapshot(WorkspaceMode::Managed, "contributor-bob"));
    select(&core);
    let mut direct = submit(&core, "direct");
    let accepted = update(
        &direct.operation,
        1,
        SessionInputState::Accepted { at_ms: 1100 },
    );
    let _effects = core
        .resolve(&mut direct, Ok(SessionResult::Input(accepted)))
        .expect("direct runtime result");
    assert_eq!(
        core.view()
            .sessions
            .mutations
            .first()
            .map(|mutation| &mutation.state),
        Some(&SessionMutationState::RuntimeReported),
        "direct runtime response does not invent a backend receipt"
    );
    let mut forwarded = submit(&core, "forwarded");
    let completed = update(
        &forwarded.operation,
        4,
        SessionInputState::Completed {
            delivery: delivery(1),
            completed_at_ms: 1500,
            outcome: SessionCompletion::Succeeded,
        },
    );
    let _watch = deliver(&core, watch, vec![SessionChange::Input(completed.clone())]);
    let receipt = received(&forwarded.operation);
    let _effects = core
        .resolve(&mut forwarded, Ok(receipt))
        .expect("delayed backend receipt");
    assert_eq!(
        core.view()
            .sessions
            .prompts
            .iter()
            .find(|prompt| prompt.input == completed.input)
            .and_then(|prompt| prompt.runtime.as_ref()),
        Some(&completed),
        "late receipt cannot overwrite confirmed completion"
    );
}

#[test]
fn a_late_transport_error_cannot_overwrite_a_recovered_durable_receipt() {
    let (core, _watch) = connected(snapshot(WorkspaceMode::Managed, "contributor-bob"));
    select(&core);
    let mut submission = submit(&core, "late");
    let mut status = request(send(&core, Event::Recover("late".into())));
    let _effects = core
        .resolve(&mut status, Ok(received(&submission.operation)))
        .expect("recovered receipt");
    let before = core.view().sessions;
    let _effects = core
        .resolve(&mut submission, Err(runtime_error()))
        .expect("late network error");
    assert_eq!(
        core.view().sessions,
        before,
        "transport failure cannot erase durable receipt"
    );
}
