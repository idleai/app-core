use super::{
    attribution, connected, deliver, mutation_id, request, requests, select, send, snapshot,
};
use crate::{
    sessions::{
        Event, SessionAcknowledgement, SessionAction, SessionCapability, SessionChange,
        SessionDraft, SessionErrorCode, SessionGrant, SessionGrantStatus, SessionInfo,
        SessionLoadState, SessionMutation, SessionMutationState, SessionPermission, SessionReceipt,
        SessionRelationship, SessionResult,
    },
    workspace::WorkspaceMode,
};

fn created_session(source: &crate::sessions::SessionSnapshot) -> SessionInfo {
    let mut info = source.sessions.first().expect("fixture session").clone();
    info.id = "allocated-session".into();
    info.title = "New runner".into();
    info.history.item = "c".repeat(64);
    info
}

#[test]
fn creation_requires_runtime_result_and_selection_is_independent() {
    for mode in [WorkspaceMode::Standalone, WorkspaceMode::Managed] {
        let source = snapshot(mode, "contributor-alice");
        let info = created_session(&source);
        let (core, _watch) = connected(source);
        select(&core);
        let mut create = request(send(
            &core,
            Event::Create {
                id: mutation_id("create"),
                draft: SessionDraft {
                    title: "New runner".into(),
                    host_id: "host-shared".into(),
                    parent: None,
                },
            },
        ));
        assert!(
            core.view()
                .sessions
                .sessions
                .iter()
                .all(|view| view.session.id != info.id),
            "pending intent does not invent a running session"
        );
        assert_eq!(
            create
                .operation
                .coordination_command()
                .expect("command conversion"),
            None,
            "directory registration alone cannot implement runtime creation"
        );
        let result = SessionResult::Created {
            request: attribution(&create.operation).key(),
            session: info.clone(),
        };
        let _effects = core
            .resolve(&mut create, Ok(result))
            .expect("runtime created and registered runner");
        let view = core.view().sessions;
        assert_eq!(
            view.selected.as_deref(),
            Some("session-shared"),
            "late creation cannot steal selection"
        );
        assert!(view.sessions.iter().any(|view| view.session == info && view.relationship == SessionRelationship::Owned), "created runner is owned by actual creator");
        assert!(
            view.prompts.is_empty(),
            "creation implies no prompt acceptance or execution"
        );
        let _effects = send(&core, Event::Select(Some(info.id)));
        assert_eq!(
            core.view()
                .sessions
                .selected_history
                .map(|binding| binding.item),
            Some("c".repeat(64)),
            "new selection exposes runtime-allocated logical item"
        );
    }
}

#[test]
fn invalid_create_responses_cannot_publish_session_identity() {
    for case in 0..4 {
        let source = snapshot(WorkspaceMode::Managed, "contributor-alice");
        let mut info = created_session(&source);
        let (core, _watch) = connected(source);
        let mut create = request(send(
            &core,
            Event::Create {
                id: mutation_id("create"),
                draft: SessionDraft {
                    title: "New runner".into(),
                    host_id: "host-shared".into(),
                    parent: None,
                },
            },
        ));
        let key = attribution(&create.operation).key();
        let result = match case {
            0 => super::received(&create.operation),
            1 => {
                info.owner = "contributor-bob".into();
                SessionResult::Created {
                    request: key,
                    session: info,
                }
            }
            2 => {
                info.history.item = "session-id-is-not-an-item".into();
                SessionResult::Created {
                    request: key,
                    session: info,
                }
            }
            _ => {
                info.history.item = "a".repeat(64);
                SessionResult::Created {
                    request: key,
                    session: info,
                }
            }
        };
        let _effects = core
            .resolve(&mut create, Ok(result))
            .expect("invalid create response");
        assert!(
            matches!(
                core.view()
                    .sessions
                    .mutations
                    .first()
                    .map(|value| &value.state),
                Some(SessionMutationState::Failed(_))
            ),
            "invalid identity/stage fails creation"
        );
        assert!(
            core.view()
                .sessions
                .sessions
                .iter()
                .all(|view| view.session.id != "allocated-session"),
            "invalid response changes no directory entries"
        );
    }
}

#[test]
fn delayed_creation_success_preserves_newer_or_omitted_directory_records() {
    for case in 0..4 {
        let source = snapshot(WorkspaceMode::Managed, "contributor-alice");
        let created = created_session(&source);
        let mut latest = created.clone();
        if case >= 2 {
            latest.revision = 2;
            latest.title = "Updated by another client".into();
            latest.runtime.runtime_id = "runtime-moved".into();
            latest.runtime.host_id = "host-moved".into();
        }
        let (core, watch) = connected(source.clone());
        let mut create = request(send(
            &core,
            Event::Create {
                id: mutation_id("create"),
                draft: SessionDraft {
                    title: "New runner".into(),
                    host_id: "host-shared".into(),
                    parent: None,
                },
            },
        ));
        let _watch = deliver(
            &core,
            watch,
            vec![
                SessionChange::Session(created.clone()),
                SessionChange::Session(latest.clone()),
            ],
        );
        let omitted = case % 2 == 1;
        if omitted {
            let mut without_created = source.clone();
            without_created.cursor.position = 100;
            let mut load = request(send(&core, Event::Refresh));
            let _watch = request(
                core.resolve(
                    &mut load,
                    Ok(SessionResult::Snapshot(Box::new(without_created))),
                )
                .expect("directory no longer includes the created session"),
            );
        }
        let result = SessionResult::Created {
            request: attribution(&create.operation).key(),
            session: created.clone(),
        };
        let _effects = core
            .resolve(&mut create, Ok(result))
            .expect("late creation result");
        let view = core.view().sessions;
        assert_eq!(
            view.mutations.first().map(|mutation| &mutation.state),
            Some(&SessionMutationState::Created(created.clone())),
            "a delayed acknowledgement still records successful creation"
        );
        assert_eq!(
            view.sessions
                .iter()
                .find(|view| view.session.id == created.id)
                .map(|view| &view.session),
            (!omitted).then_some(&latest),
            "an old creation result cannot overwrite or republish directory data"
        );
        if case >= 2 {
            let mut stale = source;
            stale.sessions.push(created);
            stale.cursor.position = 101;
            let mut load = request(send(&core, Event::Refresh));
            let _effects = core
                .resolve(&mut load, Ok(SessionResult::Snapshot(Box::new(stale))))
                .expect("old directory snapshot");
            assert!(
                matches!(core.view().sessions.load, SessionLoadState::Failed(_)),
                "late creation cannot lower the known metadata revision"
            );
        }
    }
}

#[test]
fn delayed_creation_rejects_conflicting_revisions_and_rebound_logical_items() {
    for case in 0..2 {
        let source = snapshot(WorkspaceMode::Managed, "contributor-alice");
        let mut created = created_session(&source);
        let mut known = created.clone();
        if case == 0 {
            known.title = "Conflicting title at the same revision".into();
        } else {
            known.revision = 2;
            created.history.item = "d".repeat(64);
        }
        let (core, watch) = connected(source);
        let mut create = request(send(
            &core,
            Event::Create {
                id: mutation_id("create"),
                draft: SessionDraft {
                    title: "New runner".into(),
                    host_id: "host-shared".into(),
                    parent: None,
                },
            },
        ));
        let _watch = deliver(&core, watch, vec![SessionChange::Session(known)]);
        let before = core.view().sessions.sessions;
        let result = SessionResult::Created {
            request: attribution(&create.operation).key(),
            session: created,
        };
        let _effects = core
            .resolve(&mut create, Ok(result))
            .expect("conflicting creation result");
        assert!(
            matches!(
                core.view()
                    .sessions
                    .mutations
                    .first()
                    .map(|mutation| &mutation.state),
                Some(SessionMutationState::Failed(_))
            ),
            "acknowledgements still reject conflicting metadata"
        );
        assert_eq!(
            core.view().sessions.sessions,
            before,
            "directory remains unchanged"
        );
    }
}

#[test]
fn sharing_commit_is_not_an_optimistic_grant_and_revocation_uses_current_revision() {
    let source = snapshot(WorkspaceMode::Managed, "contributor-alice");
    let mut through = source.cursor.clone();
    through.position = through.position.checked_add(1).expect("fixture cursor");
    let (core, watch) = connected(source);
    select(&core);
    let mut invitation = request(send(
        &core,
        Event::Invite {
            id: mutation_id("invite"),
            grant_id: "grant-new".into(),
            grantee: "contributor-bob".into(),
            permissions: vec![SessionPermission::Observe],
            expires_at_ms: Some(1500),
        },
    ));
    let grant = SessionGrant {
        id: "grant-new".into(),
        session_id: "session-shared".into(),
        grantee: "contributor-bob".into(),
        granted_by: "contributor-alice".into(),
        permissions: vec![SessionPermission::Observe],
        expires_at_ms: Some(1500),
        revision: 1,
        status: SessionGrantStatus::Active,
    };
    let commit = SessionResult::Acknowledged(SessionAcknowledgement::Committed {
        receipt: SessionReceipt {
            request: attribution(&invitation.operation).key(),
            received_at_ms: 1000,
            retry_until_ms: 2500,
        },
        committed_at_ms: 1000,
        through,
    });
    let _effects = core
        .resolve(&mut invitation, Ok(commit))
        .expect("sharing committed");
    assert!(
        core.view()
            .sessions
            .sessions
            .iter()
            .flat_map(|view| &view.grants)
            .all(|value| value.id != "grant-new"),
        "commit alone cannot advance recovered participation"
    );
    let _watch = deliver(&core, watch, vec![SessionChange::Grant(grant)]);
    let revoke = request(send(
        &core,
        Event::Revoke {
            id: mutation_id("revoke"),
            grant_id: "grant-new".into(),
        },
    ));
    assert!(
        matches!(&revoke.operation.action, SessionAction::Mutate { mutation: SessionMutation::Revoke { expected_revision: 1, grant_id, .. }, .. } if grant_id == "grant-new"),
        "revocation uses confirmed grant revision"
    );
}

#[test]
fn ownership_participation_and_compute_capabilities_remain_separate() {
    let (bob, _watch) = connected(snapshot(WorkspaceMode::Managed, "contributor-bob"));
    select(&bob);
    let view = bob.view().sessions;
    assert_eq!(
        view.sessions.len(),
        1,
        "Control directory entry is not an invitation to Bob"
    );
    assert_eq!(
        view.sessions.first().map(|view| view.relationship),
        Some(SessionRelationship::Invited),
        "owner remains Alice"
    );
    assert!(
        view.sessions.first().is_some_and(|view| view
            .actions
            .contains(&SessionPermission::SubmitInput)
            && !view.actions.contains(&SessionPermission::Invite)),
        "input permission does not imply sharing authority"
    );
    assert!(
        requests(send(
            &bob,
            Event::Invite {
                id: mutation_id("forbidden-share"),
                grant_id: "g".into(),
                grantee: "contributor-alice".into(),
                permissions: vec![SessionPermission::SubmitInput],
                expires_at_ms: None
            }
        ))
        .is_empty(),
        "invited input contributor cannot implicitly reshare"
    );
    let mut source = snapshot(WorkspaceMode::Managed, "contributor-alice");
    source
        .grants
        .retain(|grant| grant.grantee != "contributor-alice");
    let (alice, _watch) = connected(source);
    select(&alice);
    assert!(
        requests(send(
            &alice,
            Event::Submit {
                id: mutation_id("owner-input"),
                text: "prompt".into()
            }
        ))
        .is_empty(),
        "directory ownership alone is not a runtime input grant"
    );
}

#[test]
fn disconnected_runtime_capabilities_stay_unavailable_and_fail_visibly() {
    let mut source = snapshot(WorkspaceMode::Managed, "contributor-alice");
    source.capabilities = crate::sessions::SessionCapabilities::default();
    let (core, _watch) = connected(source);
    assert_eq!(
        core.view().sessions.capabilities.create,
        SessionCapability::Unavailable,
        "metadata does not claim runtime availability"
    );
    assert!(
        requests(send(
            &core,
            Event::Create {
                id: mutation_id("create"),
                draft: SessionDraft {
                    title: "New".into(),
                    host_id: "host-shared".into(),
                    parent: None
                }
            }
        ))
        .is_empty(),
        "no synthetic runtime effect when unavailable"
    );
    assert_eq!(
        core.view().sessions.action_error.map(|error| error.code),
        Some(SessionErrorCode::UnsupportedOperation),
        "unavailable operation is explicit"
    );
}

#[test]
fn grant_retry_keeps_original_intent_after_its_change_event_arrives() {
    let (core, watch) = connected(snapshot(WorkspaceMode::Managed, "contributor-alice"));
    select(&core);
    let mut invitation = request(send(
        &core,
        Event::Invite {
            id: mutation_id("retry-share"),
            grant_id: "retry-grant".into(),
            grantee: "contributor-bob".into(),
            permissions: vec![SessionPermission::Observe],
            expires_at_ms: Some(1500),
        },
    ));
    let original = invitation.operation.clone();
    let _effects = core
        .resolve(&mut invitation, Err(super::runtime_error()))
        .expect("retryable receipt failure");
    let grant = SessionGrant {
        id: "retry-grant".into(),
        session_id: "session-shared".into(),
        grantee: "contributor-bob".into(),
        granted_by: "contributor-alice".into(),
        permissions: vec![SessionPermission::Observe],
        expires_at_ms: Some(1500),
        revision: 1,
        status: SessionGrantStatus::Active,
    };
    let _watch = deliver(&core, watch, vec![SessionChange::Grant(grant)]);
    let _effects = send(&core, Event::Tick(1600));
    let retry = request(send(&core, Event::Retry("retry-share".into())));
    assert_eq!(
        retry.operation, original,
        "retry replays the original request even when its grant already exists or expired"
    );
}
