use super::{changes, connected, deliver, request, select, send, snapshot, submit, update};
use crate::{
    Core,
    sessions::{
        Event, SessionChange, SessionCompletion, SessionDelivery, SessionErrorCode,
        SessionInputState, SessionLoadState, SessionMutationState, SessionResult, SessionSnapshot,
    },
    workspace::WorkspaceMode,
};

fn relocated(mode: WorkspaceMode) -> (Core, SessionSnapshot) {
    let mut source = snapshot(mode, "contributor-bob");
    let (core, watch) = connected(source.clone());
    select(&core);
    let submission = submit(&core, "terminal-input");
    let completed = update(
        &submission.operation,
        4,
        SessionInputState::Completed {
            delivery: SessionDelivery {
                accepted_at_ms: 1100,
                ordered_at_ms: 1150,
                order: 1,
            },
            completed_at_ms: 1300,
            outcome: SessionCompletion::Succeeded,
        },
    );
    let moved = source.sessions.first_mut().expect("session");
    moved.revision = 2;
    moved.runtime.runtime_id = "runtime-moved".into();
    moved.runtime.host_id = "host-moved".into();
    let _watch = deliver(
        &core,
        watch,
        vec![
            SessionChange::Input(completed.clone()),
            SessionChange::Session(moved.clone()),
        ],
    );
    source.inputs = vec![completed];
    source.cursor.position = 100;
    source.now_ms = 1500;
    (core, source)
}

#[test]
fn fresh_and_reconnecting_clients_recover_terminal_facts_after_runtime_relocation() {
    for mode in [WorkspaceMode::Standalone, WorkspaceMode::Managed] {
        let (existing, source) = relocated(mode);
        for core in [Core::new(), existing] {
            let _effects = send(&core, Event::Disconnect);
            let mut load = request(send(&core, Event::Connect(source.context.clone())));
            let _watch = request(
                core.resolve(
                    &mut load,
                    Ok(SessionResult::Snapshot(Box::new(source.clone()))),
                )
                .expect("snapshot response"),
            );
            let view = core.view().sessions;
            assert_eq!(view.load, SessionLoadState::Ready, "recovery succeeds");
            assert_eq!(
                view.prompts
                    .first()
                    .and_then(|prompt| prompt.runtime.as_ref()),
                source.inputs.first(),
                "recovery retains the original runtime's terminal fact"
            );
            assert_eq!(
                view.sessions.first().map(|view| &view.session),
                source.sessions.first(),
                "directory still routes new input to the relocated runtime"
            );
            assert!(
                view.prompts.iter().all(|prompt| prompt.text.is_none()),
                "a new connection does not invent or inherit local prompt text"
            );
        }
    }
}

#[test]
fn recovered_history_does_not_authorize_new_reports_from_a_retired_runtime() {
    let (_original, source) = relocated(WorkspaceMode::Managed);
    for case in 0..3 {
        let (core, mut watch) = connected(source.clone());
        let mut stale = source.inputs.first().expect("retained input").clone();
        stale.revision = 5;
        match case {
            0 => {
                let result = changes(&watch, vec![SessionChange::Input(stale)]);
                let _effects = core.resolve(&mut watch, Ok(result)).expect("watch reply");
                assert!(
                    matches!(core.view().sessions.updates, SessionLoadState::Failed(_)),
                    "watch cannot introduce new facts from a retired runtime"
                );
            }
            1 => {
                let mut query = request(send(&core, Event::RefreshInput(stale.input.clone())));
                let _effects = core
                    .resolve(&mut query, Ok(SessionResult::Input(stale)))
                    .expect("status reply");
                assert_eq!(
                    core.view().sessions.action_error.map(|error| error.code),
                    Some(SessionErrorCode::InvalidRequest),
                    "a status reply cannot introduce new facts from a retired runtime"
                );
            }
            _ => {
                select(&core);
                let mut submission = submit(&core, "new-input");
                let stale = update(
                    &submission.operation,
                    1,
                    SessionInputState::Accepted { at_ms: 1600 },
                );
                let _effects = core
                    .resolve(&mut submission, Ok(SessionResult::Input(stale)))
                    .expect("direct runtime reply");
                assert!(
                    matches!(
                        core.view()
                            .sessions
                            .mutations
                            .first()
                            .map(|value| &value.state),
                        Some(SessionMutationState::Failed(_))
                    ),
                    "the retired runtime cannot accept new submissions"
                );
            }
        }
        assert_eq!(
            core.view()
                .sessions
                .prompts
                .first()
                .and_then(|prompt| prompt.runtime.as_ref()),
            source.inputs.first(),
            "rejected reports preserve the recovered terminal result"
        );
    }
}

#[test]
fn relocated_snapshots_still_reject_invalid_input_scope_identity_revision_and_order() {
    let (_original, source) = relocated(WorkspaceMode::Managed);
    for case in 0..7 {
        let mut invalid = source.clone();
        let input = invalid.inputs.first_mut().expect("retained input");
        match case {
            0 => input.runtime_id.clear(),
            1 => input.input.request.workspace_id = "other-workspace".into(),
            2 => input.contributor.contributor_id = "contributor-alice".into(),
            3 => input.revision = 0,
            4 => {
                input.state = SessionInputState::Ordered(SessionDelivery {
                    accepted_at_ms: 1100,
                    ordered_at_ms: 1150,
                    order: 0,
                });
            }
            _ => {
                let mut duplicate = input.clone();
                if case == 5 {
                    duplicate.revision = 5;
                } else {
                    duplicate.input.request.request_id = "duplicate-order".into();
                }
                invalid.inputs.push(duplicate);
            }
        }
        let core = Core::new();
        let mut load = request(send(&core, Event::Connect(invalid.context.clone())));
        let _effects = core
            .resolve(&mut load, Ok(SessionResult::Snapshot(Box::new(invalid))))
            .expect("invalid snapshot reply");
        assert!(
            matches!(core.view().sessions.load, SessionLoadState::Failed(_)),
            "case {case} must fail snapshot validation"
        );
        assert!(
            core.view().sessions.prompts.is_empty(),
            "no partial recovery"
        );
    }
}
