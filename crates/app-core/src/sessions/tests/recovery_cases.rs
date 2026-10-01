use super::{
    changes, connected, deliver, received, request, select, send, snapshot, submit, update,
};
use crate::{
    Core,
    sessions::{
        Event, SessionChange, SessionCompletion, SessionDelivery, SessionGrantStatus,
        SessionInputState, SessionLoadState, SessionResult,
    },
    workspace::{MemberStatus, WorkspaceMode},
};

#[test]
fn same_context_refresh_preserves_selection_pending_text_and_confirmed_facts() {
    let source = snapshot(WorkspaceMode::Managed, "contributor-bob");
    let (core, mut old_watch) = connected(source.clone());
    select(&core);
    let mut submission = submit(&core, "pending");
    let mut refresh = request(send(&core, Event::Refresh));
    assert_eq!(
        core.view().sessions.selected.as_deref(),
        Some("session-shared"),
        "refresh does not clear selection"
    );
    let stale = changes(&old_watch, vec![]);
    assert!(
        core.resolve(&mut old_watch, Ok(stale))
            .expect("old watch")
            .is_empty(),
        "old watch cannot resume after snapshot recovery starts"
    );
    let _watch = request(
        core.resolve(&mut refresh, Ok(SessionResult::Snapshot(Box::new(source))))
            .expect("replacement snapshot"),
    );
    assert_eq!(
        core.view().sessions.prompts.len(),
        1,
        "unreceived local prompt survives snapshot replacement"
    );
    let receipt = received(&submission.operation);
    let _effects = core
        .resolve(&mut submission, Ok(receipt))
        .expect("still-owned mutation response");
    assert_eq!(
        core.view()
            .sessions
            .prompts
            .first()
            .and_then(|prompt| prompt.text.as_deref()),
        Some("  Exact prompt\n🌍  "),
        "recovery retains exact original prompt"
    );
}

#[test]
fn reconnect_discards_previous_lifetime_results_even_when_returning_to_same_scope() {
    let source = snapshot(WorkspaceMode::Managed, "contributor-bob");
    let (core, mut old_watch) = connected(source.clone());
    select(&core);
    let mut old_submit = submit(&core, "old-prompt");
    let _effects = send(&core, Event::Disconnect);
    let mut next = request(send(&core, Event::Connect(source.context.clone())));
    let _watch = request(
        core.resolve(&mut next, Ok(SessionResult::Snapshot(Box::new(source))))
            .expect("new connection"),
    );
    let before = core.view();
    let result = received(&old_submit.operation);
    let _effects = core
        .resolve(&mut old_submit, Ok(result))
        .expect("old mutation reply");
    let result = changes(
        &old_watch,
        vec![SessionChange::Input(update(
            &old_submit.operation,
            1,
            SessionInputState::Accepted { at_ms: 1100 },
        ))],
    );
    let _effects = core
        .resolve(&mut old_watch, Ok(result))
        .expect("old runtime reply");
    assert_eq!(
        core.view(),
        before,
        "retired continuations cannot change a new lifetime"
    );
}

#[test]
fn revocation_and_expiry_hide_invited_sessions_and_retire_late_results() {
    for membership in [false, true] {
        let source = snapshot(WorkspaceMode::Managed, "contributor-bob");
        let (core, mut watch) = connected(source.clone());
        select(&core);
        let mut submission = submit(&core, "revoked");
        let change = if membership {
            let mut member = source
                .members
                .iter()
                .find(|member| member.contributor_id == "contributor-bob")
                .expect("Bob membership")
                .clone();
            member.revision = 2;
            member.status = MemberStatus::Revoked;
            SessionChange::Member(member)
        } else {
            let mut grant = source
                .grants
                .iter()
                .find(|grant| grant.grantee == "contributor-bob")
                .expect("Bob session grant")
                .clone();
            grant.revision = 2;
            grant.status = SessionGrantStatus::Revoked {
                at_ms: 1100,
                by: "contributor-alice".into(),
            };
            SessionChange::Grant(grant)
        };
        let result = changes(&watch, vec![change]);
        let _effects = core
            .resolve(&mut watch, Ok(result))
            .expect("revoked access");
        assert!(
            core.view().sessions.sessions.is_empty(),
            "revocation hides owned/invited data as appropriate"
        );
        assert!(
            core.view().sessions.selected.is_none(),
            "selection is cleared when access disappears"
        );
        assert!(
            core.view().sessions.prompts.is_empty(),
            "private prompt content is hidden"
        );
        let before = core.view();
        let result = received(&submission.operation);
        let _effects = core
            .resolve(&mut submission, Ok(result))
            .expect("late revoked response");
        assert_eq!(
            core.view(),
            before,
            "late responses cannot restore revoked data"
        );
    }
    let mut source = snapshot(WorkspaceMode::Managed, "contributor-bob");
    for grant in &mut source.grants {
        if grant.grantee == "contributor-bob" {
            grant.expires_at_ms = Some(1200);
        }
    }
    let (core, _watch) = connected(source);
    select(&core);
    let _effects = send(&core, Event::Tick(1200));
    assert!(
        core.view().sessions.sessions.is_empty(),
        "expiry is enforced without waiting for a provider event"
    );
    let _effects = send(&core, Event::Tick(1100));
    assert!(
        core.view().sessions.sessions.is_empty(),
        "clock regression cannot revive a grant"
    );
}

#[test]
fn cursor_reset_uses_snapshot_and_duplicate_prompt_order_is_rejected_atomically() {
    let source = snapshot(WorkspaceMode::Managed, "contributor-bob");
    let (core, mut watch) = connected(source.clone());
    select(&core);
    let first = submit(&core, "first");
    let second = submit(&core, "second");
    let first_fact = update(
        &first.operation,
        2,
        SessionInputState::Ordered(SessionDelivery {
            accepted_at_ms: 1100,
            ordered_at_ms: 1150,
            order: 1,
        }),
    );
    let second_fact = update(&second.operation, 2, first_fact.state.clone());
    let result = changes(
        &watch,
        vec![
            SessionChange::Input(first_fact),
            SessionChange::Input(second_fact),
        ],
    );
    let _effects = core
        .resolve(&mut watch, Ok(result))
        .expect("conflicting order page");
    assert!(
        core.view()
            .sessions
            .prompts
            .iter()
            .all(|prompt| prompt.runtime.is_none()),
        "invalid page commits no partial runtime facts"
    );
    let mut reload = request(send(&core, Event::Refresh));
    let mut watch = request(
        core.resolve(&mut reload, Ok(SessionResult::Snapshot(Box::new(source))))
            .expect("recover after conflict"),
    );
    let reset = request(
        core.resolve(&mut watch, Ok(SessionResult::SnapshotRequired))
            .expect("cursor reset"),
    );
    assert!(
        matches!(
            reset.operation.action,
            crate::sessions::SessionAction::Snapshot
        ),
        "retention reset performs authoritative recovery"
    );
    assert_eq!(
        core.view().sessions.selected.as_deref(),
        Some("session-shared"),
        "selection survives reset"
    );
    assert_eq!(
        core.view().sessions.prompts.len(),
        2,
        "pending prompts are not resent or lost"
    );
}

#[test]
fn runtime_relocation_keeps_logical_identity_and_rejects_the_old_producer() {
    let source = snapshot(WorkspaceMode::Managed, "contributor-bob");
    let mut moved = source.sessions.first().expect("session").clone();
    moved.revision = 2;
    moved.runtime.runtime_id = "runtime-moved".into();
    moved.runtime.host_id = "host-moved".into();
    let (core, watch) = connected(source);
    select(&core);
    let submission = submit(&core, "moving-input");
    let accepted = update(
        &submission.operation,
        1,
        SessionInputState::Accepted { at_ms: 1100 },
    );
    let watch = deliver(
        &core,
        watch,
        vec![
            SessionChange::Input(accepted),
            SessionChange::Session(moved),
        ],
    );
    assert_eq!(
        core.view()
            .sessions
            .selected_history
            .as_ref()
            .map(|binding| binding.item.clone()),
        Some("a".repeat(64)),
        "relocation does not allocate a new logical session"
    );
    let mut completed = update(
        &submission.operation,
        4,
        SessionInputState::Completed {
            delivery: SessionDelivery {
                accepted_at_ms: 1100,
                order: 1,
                ordered_at_ms: 1150,
            },
            completed_at_ms: 1300,
            outcome: SessionCompletion::Succeeded,
        },
    );
    completed.runtime_id = "runtime-moved".into();
    let mut watch = deliver(&core, watch, vec![SessionChange::Input(completed.clone())]);
    let mut stale_producer = completed.clone();
    stale_producer.runtime_id = "runtime-evo".into();
    stale_producer.revision = 5;
    let result = changes(&watch, vec![SessionChange::Input(stale_producer)]);
    let _effects = core
        .resolve(&mut watch, Ok(result))
        .expect("retired runtime response");
    assert!(
        matches!(core.view().sessions.updates, SessionLoadState::Failed(_)),
        "old producer cannot publish new revisions"
    );
    assert_eq!(
        core.view()
            .sessions
            .prompts
            .first()
            .and_then(|prompt| prompt.runtime.as_ref()),
        Some(&completed),
        "last authenticated result is retained"
    );
}

#[test]
fn snapshots_cannot_infer_or_rebind_items_and_audience_switch_drops_private_input() {
    let source = snapshot(WorkspaceMode::Managed, "contributor-bob");
    let (core, _watch) = connected(source.clone());
    select(&core);
    let _submission = submit(&core, "private");
    let mut changed = source.clone();
    changed.sessions.first_mut().expect("session").history.item = "c".repeat(64);
    changed.sessions.first_mut().expect("session").revision = 2;
    let mut load = request(send(&core, Event::Refresh));
    let _effects = core
        .resolve(&mut load, Ok(SessionResult::Snapshot(Box::new(changed))))
        .expect("bad binding");
    assert!(
        matches!(core.view().sessions.load, SessionLoadState::Failed(_)),
        "logical session mapping is immutable"
    );
    let alice = snapshot(WorkspaceMode::Managed, "contributor-alice");
    let mut load = request(send(&core, Event::Connect(alice.context.clone())));
    let _watch = request(
        core.resolve(&mut load, Ok(SessionResult::Snapshot(Box::new(alice))))
            .expect("new audience snapshot"),
    );
    assert!(
        core.view().sessions.prompts.is_empty(),
        "another audience cannot inherit locally submitted text"
    );
    assert!(
        core.view().sessions.mutations.is_empty(),
        "another audience cannot retry old mutations"
    );
}

#[test]
fn foreign_snapshot_cannot_bind_a_new_context() {
    let source = snapshot(WorkspaceMode::Managed, "contributor-bob");
    let core = Core::new();
    let mut load = request(send(&core, Event::Connect(source.context)));
    let wrong = snapshot(WorkspaceMode::Managed, "contributor-alice");
    let _effects = core
        .resolve(&mut load, Ok(SessionResult::Snapshot(Box::new(wrong))))
        .expect("foreign snapshot response");
    assert!(
        matches!(core.view().sessions.load, SessionLoadState::Failed(_)),
        "audience mismatch is rejected"
    );
    assert!(
        core.view().sessions.sessions.is_empty(),
        "foreign metadata never enters the view"
    );
}

#[test]
fn omitted_grant_tombstones_and_revoked_memberships_cannot_reappear_from_old_snapshots() {
    for membership in [false, true] {
        let original = snapshot(WorkspaceMode::Managed, "contributor-bob");
        let (core, mut watch) = connected(original.clone());
        let change = if membership {
            let mut record = original
                .members
                .iter()
                .find(|member| member.contributor_id == "contributor-bob")
                .expect("member")
                .clone();
            record.revision = 2;
            record.status = MemberStatus::Revoked;
            SessionChange::Member(record)
        } else {
            let mut record = original
                .grants
                .iter()
                .find(|grant| grant.grantee == "contributor-bob")
                .expect("grant")
                .clone();
            record.revision = 2;
            record.status = SessionGrantStatus::Revoked {
                at_ms: 1100,
                by: "contributor-alice".into(),
            };
            SessionChange::Grant(record)
        };
        let result = changes(&watch, vec![change]);
        let _effects = core.resolve(&mut watch, Ok(result)).expect("revocation");
        let mut stale = original.clone();
        stale.cursor.position = 100;
        if !membership {
            let mut omitted = stale.clone();
            omitted
                .grants
                .retain(|grant| grant.grantee != "contributor-bob");
            let mut load = request(send(&core, Event::Refresh));
            let _effects = core
                .resolve(&mut load, Ok(SessionResult::Snapshot(Box::new(omitted))))
                .expect("snapshot omitting tombstone");
        }
        let mut load = request(send(&core, Event::Refresh));
        let _effects = core
            .resolve(&mut load, Ok(SessionResult::Snapshot(Box::new(stale))))
            .expect("stale snapshot");
        assert!(
            matches!(core.view().sessions.load, SessionLoadState::Failed(_)),
            "known revoked record cannot regress after snapshot omission"
        );
        assert!(
            core.view().sessions.sessions.is_empty(),
            "old records cannot restore access"
        );
    }
}
