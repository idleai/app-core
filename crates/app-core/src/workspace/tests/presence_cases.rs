use super::{directory, failure, info, one, presence, select, send, snapshot};
use crate::{
    Core,
    workspace::{
        Event, MemberStatus, PresenceSnapshot, PresenceStatus, WorkspaceErrorKind, WorkspaceMode,
        WorkspaceRequestState, WorkspaceResult,
    },
};

#[test]
fn fresh_connections_preserve_identity_and_expire_in_both_modes() {
    for mode in [WorkspaceMode::Standalone, WorkspaceMode::Managed] {
        let core = Core::new();
        let a = info("a", mode);
        let _effects = directory(&core, vec![a.clone()]);
        let mut request = select(&core, &a, "alice");
        assert_eq!(
            core.view()
                .workspace
                .members
                .first()
                .expect("membership")
                .presence,
            PresenceStatus::Unknown,
            "membership does not imply online"
        );
        let mut away = presence("alice", "phone", PresenceStatus::Away);
        away.valid_until_ms = 300;
        let entries = vec![presence("alice", "laptop", PresenceStatus::Online), away];
        let _effects = core
            .resolve(
                &mut request,
                Ok(WorkspaceResult::Presence(PresenceSnapshot {
                    workspace_id: "a".into(),
                    as_of_ms: 120,
                    entries,
                })),
            )
            .expect("presence response");
        let members = core.view().workspace.members;
        let member = members.first().expect("member");
        assert_eq!(
            member.presence,
            PresenceStatus::Online,
            "any active connection means online"
        );
        assert_eq!(
            member.connections.len(),
            2,
            "multiple connections are preserved"
        );
        assert_eq!(
            member.member.display_name, "Person alice",
            "human identity distinct from host label"
        );
        assert_eq!(
            member
                .connections
                .first()
                .expect("connection")
                .host_id
                .as_deref(),
            Some("shared-host"),
            "host location retained separately"
        );
        let _effects = send(&core, Event::Tick(200));
        assert_eq!(
            core.view()
                .workspace
                .members
                .first()
                .expect("member")
                .presence,
            PresenceStatus::Away,
            "exclusive expiry removes online connection"
        );
        let _effects = send(&core, Event::Tick(300));
        let expired = core.view().workspace.members;
        assert_eq!(
            expired.first().expect("member").presence,
            PresenceStatus::Unknown,
            "expiry is unknown, not an invented disconnect"
        );
        assert!(
            expired.first().expect("member").connections.is_empty(),
            "expired locations are hidden"
        );
        let _effects = send(&core, Event::Tick(100));
        assert_eq!(
            core.view().workspace.members,
            expired,
            "clock rollback cannot resurrect presence"
        );
    }
}

#[test]
fn explicit_offline_hides_locations_and_refresh_replaces_connections() {
    let core = Core::new();
    let a = info("a", WorkspaceMode::Managed);
    let _effects = directory(&core, vec![a.clone()]);
    let mut request = select(&core, &a, "alice");
    let offline = presence("alice", "laptop", PresenceStatus::Offline);
    let _effects = core
        .resolve(
            &mut request,
            Ok(WorkspaceResult::Presence(PresenceSnapshot {
                workspace_id: "a".into(),
                as_of_ms: 100,
                entries: vec![offline],
            })),
        )
        .expect("disconnect observation");
    let members = core.view().workspace.members;
    let member = members.first().expect("member");
    assert_eq!(
        member.presence,
        PresenceStatus::Offline,
        "explicit offline state preserved"
    );
    assert!(
        member.connections.is_empty(),
        "offline locations are hidden"
    );
    let mut refresh = one(send(&core, Event::RefreshPresence));
    let _effects = core
        .resolve(
            &mut refresh,
            Ok(WorkspaceResult::Presence(PresenceSnapshot {
                workspace_id: "a".into(),
                as_of_ms: 110,
                entries: Vec::new(),
            })),
        )
        .expect("empty presence snapshot");
    assert_eq!(
        core.view()
            .workspace
            .members
            .first()
            .expect("member")
            .presence,
        PresenceStatus::Unknown,
        "absence replaces old observations without implying offline"
    );
    assert_eq!(
        core.view().workspace.presence_state,
        WorkspaceRequestState::Ready,
        "empty result is a known successful read"
    );
}

#[test]
fn revoked_members_cannot_regain_presence_from_old_or_current_provider_results() {
    let core = Core::new();
    let a = info("a", WorkspaceMode::Managed);
    let _effects = directory(&core, vec![a.clone()]);
    let mut old = select(&core, &a, "alice");
    let mut refresh = one(send(&core, Event::RefreshWorkspace));
    let mut revoked = snapshot(a.clone(), "alice");
    let member = revoked.members.first_mut().expect("member");
    member.status = MemberStatus::Revoked;
    member.revision = 2;
    let mut fresh = one(core
        .resolve(&mut refresh, Ok(WorkspaceResult::Snapshot(revoked)))
        .expect("membership revocation"));
    let stale = WorkspaceResult::Presence(PresenceSnapshot {
        workspace_id: "a".into(),
        as_of_ms: 100,
        entries: vec![presence("alice", "laptop", PresenceStatus::Online)],
    });
    assert!(
        core.resolve(&mut old, Ok(stale.clone()))
            .expect("old presence")
            .is_empty(),
        "metadata refresh invalidates pending presence"
    );
    let _effects = core
        .resolve(&mut fresh, Ok(stale))
        .expect("presence source still catching up");
    let members = core.view().workspace.members;
    assert_eq!(
        members.first().expect("revoked member").presence,
        PresenceStatus::Unknown,
        "revocation overrides observation"
    );
    assert!(
        members
            .first()
            .expect("revoked member")
            .connections
            .is_empty(),
        "revoked locations suppressed"
    );
    let mut refresh = one(send(&core, Event::RefreshWorkspace));
    let _effects = core
        .resolve(
            &mut refresh,
            Ok(WorkspaceResult::Snapshot(snapshot(a, "alice"))),
        )
        .expect("regressed membership");
    assert_eq!(
        core.view()
            .workspace
            .members
            .first()
            .expect("member")
            .member
            .status,
        MemberStatus::Revoked,
        "older membership cannot restore active state"
    );
    assert!(
        matches!(
            core.view().workspace.snapshot_state,
            WorkspaceRequestState::Failed(_)
        ),
        "revision regression explicit"
    );
}

#[test]
fn malformed_or_foreign_presence_never_appears_in_member_views() {
    let valid = presence("alice", "laptop", PresenceStatus::Online);
    let mut invalid = Vec::new();
    let mut foreign_member = valid.clone();
    foreign_member.contributor_id = "outsider".into();
    invalid.push(vec![foreign_member]);
    let mut foreign_repo = valid.clone();
    foreign_repo.repository_id = Some("private-repository".into());
    invalid.push(vec![foreign_repo]);
    let mut foreign_host = valid.clone();
    foreign_host.host_id = Some("private-host".into());
    invalid.push(vec![foreign_host]);
    let mut missing_repo = valid.clone();
    missing_repo.repository_id = None;
    invalid.push(vec![missing_repo]);
    let mut bad_expiry = valid.clone();
    bad_expiry.valid_until_ms = 99;
    invalid.push(vec![bad_expiry]);
    let mut future = valid.clone();
    future.observed_at_ms = 101;
    invalid.push(vec![future]);
    invalid.push(vec![valid.clone(), valid]);
    for entries in invalid {
        let core = Core::new();
        let a = info("a", WorkspaceMode::Managed);
        let _effects = directory(&core, vec![a.clone()]);
        let mut request = select(&core, &a, "alice");
        let _effects = core
            .resolve(
                &mut request,
                Ok(WorkspaceResult::Presence(PresenceSnapshot {
                    workspace_id: "a".into(),
                    as_of_ms: 100,
                    entries,
                })),
            )
            .expect("invalid provider response");
        assert!(
            matches!(
                core.view().workspace.presence_state,
                WorkspaceRequestState::Failed(_)
            ),
            "invalid presence rejected"
        );
        assert_eq!(
            core.view()
                .workspace
                .members
                .first()
                .expect("member")
                .presence,
            PresenceStatus::Unknown,
            "no invalid observations exposed"
        );
    }
}

#[test]
fn presence_errors_are_independent_retryable_and_clear_stale_locations() {
    let core = Core::new();
    let a = info("a", WorkspaceMode::Standalone);
    let _effects = directory(&core, vec![a.clone()]);
    let mut request = select(&core, &a, "alice");
    let _effects = core
        .resolve(
            &mut request,
            Ok(WorkspaceResult::Presence(PresenceSnapshot {
                workspace_id: "a".into(),
                as_of_ms: 100,
                entries: vec![presence("alice", "laptop", PresenceStatus::Online)],
            })),
        )
        .expect("presence");
    let mut refresh = one(send(&core, Event::RefreshPresence));
    let error = failure(WorkspaceErrorKind::Unsupported);
    let _effects = core
        .resolve(&mut refresh, Err(error.clone()))
        .expect("unavailable capability");
    assert_eq!(
        core.view().workspace.snapshot_state,
        WorkspaceRequestState::Ready,
        "metadata remains ready"
    );
    assert_eq!(
        core.view().workspace.presence_state,
        WorkspaceRequestState::Failed(error),
        "presence has independent failure"
    );
    assert_eq!(
        core.view()
            .workspace
            .members
            .first()
            .expect("member")
            .presence,
        PresenceStatus::Unknown,
        "failed refresh hides locations"
    );
    let mut retry = one(send(&core, Event::RefreshPresence));
    let _effects = core
        .resolve(
            &mut retry,
            Ok(WorkspaceResult::Presence(PresenceSnapshot {
                workspace_id: "foreign".into(),
                as_of_ms: 100,
                entries: Vec::new(),
            })),
        )
        .expect("foreign scope");
    assert!(
        matches!(
            core.view().workspace.presence_state,
            WorkspaceRequestState::Failed(_)
        ),
        "scope mismatch remains an explicit error"
    );
}
