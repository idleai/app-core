use idle_protocol::v1::{
    Record,
    api::{ApiError, ErrorCode, RetryAdvice},
    control::ControlOwnership,
    events::{RecoveryCursor, RecoverySnapshot},
    identity::{Contributor, ControlEpoch, EventPosition, Revision},
    membership::{Membership, MembershipStatus, Role},
    resources::{Availability, ComputeHost, Health, ModelProvider, ProviderKind},
    workspace::{CoordinationMode, Repository, Workspace},
};

use crate::workspace::{
    MemberRole, MemberStatus, WorkspaceError, WorkspaceErrorKind, WorkspaceMode, WorkspaceSnapshot,
};

fn record<T>(value: T) -> Record<T> {
    Record {
        revision: Revision(u64::MAX),
        value,
    }
}

fn source(mode: CoordinationMode) -> RecoverySnapshot {
    RecoverySnapshot {
        as_of: RecoveryCursor {
            workspace_id: "workspace".into(),
            contributor_id: "alice".into(),
            stream_id: "stream".into(),
            position: EventPosition(100),
        },
        workspace: record(Workspace {
            id: "workspace".into(),
            name: "Workspace".into(),
            chain: "logical-chain".into(),
            mode,
        }),
        contributors: vec![record(Contributor {
            id: "alice".into(),
            display_name: "Alice".into(),
            identities: Vec::new(),
        })],
        memberships: vec![
            record(Membership {
                contributor_id: "alice".into(),
                role: Role::Admin,
                status: MembershipStatus::Active,
            }),
            record(Membership {
                contributor_id: "missing-directory-entry".into(),
                role: Role::Viewer,
                status: MembershipStatus::Revoked,
            }),
        ],
        invitations: Vec::new(),
        sessions: Vec::new(),
        hosts: Vec::new(),
        providers: Vec::new(),
        models: Vec::new(),
        grants: Vec::new(),
        control: ControlOwnership {
            workspace_id: "workspace".into(),
            last_epoch: ControlEpoch(0),
            lease: None,
        },
        inputs: Vec::new(),
    }
}

#[test]
fn protocol_snapshots_project_both_modes_without_host_identity_or_grant_inference() {
    let repository = Repository {
        id: "repository".into(),
        name: "Repository".into(),
        remote: None,
    };
    for mode in [
        CoordinationMode::Standalone {
            repository: repository.clone(),
        },
        CoordinationMode::Managed {
            repositories: vec![repository.clone()],
        },
    ] {
        let mut source = source(mode);
        let health = Health {
            availability: Availability::Available,
            observed_at: idle_protocol::v1::identity::Timestamp(1),
            valid_until: idle_protocol::v1::identity::Timestamp(2),
        };
        source.hosts.push(record(ComputeHost {
            id: "host".into(),
            owner: "owner".into(),
            name: "Host owner label".into(),
            capabilities: Vec::new(),
            health: health.clone(),
            routes: Vec::new(),
        }));
        source.providers.push(record(ModelProvider {
            id: "provider".into(),
            owner: "owner".into(),
            name: "Provider".into(),
            kind: ProviderKind::External,
            health,
            routes: Vec::new(),
        }));
        let view = WorkspaceSnapshot::try_from(&source).expect("protocol snapshot projection");
        assert_eq!(
            view.workspace.chain, "logical-chain",
            "logical reference preserved"
        );
        assert_eq!(
            view.workspace.revision,
            u64::MAX,
            "full-width revisions preserved"
        );
        assert_eq!(
            view.workspace.repositories.first().expect("repository").id,
            "repository",
            "repository identity preserved"
        );
        assert_eq!(
            view.workspace.mode,
            if matches!(
                source.workspace.value.mode,
                CoordinationMode::Standalone { .. }
            ) {
                WorkspaceMode::Standalone
            } else {
                WorkspaceMode::Managed
            },
            "mode preserved"
        );
        let alice = view.members.first().expect("member");
        assert_eq!(
            (alice.display_name.as_str(), alice.role),
            ("Alice", MemberRole::Admin),
            "human name is not the host owner"
        );
        let missing = view.members.last().expect("revoked member");
        assert_eq!(
            missing.display_name, missing.contributor_id,
            "missing directory metadata has a stable fallback"
        );
        assert_eq!(missing.status, MemberStatus::Revoked, "revocation retained");
        assert_eq!(
            view.host_ids,
            ["host"],
            "only workspace host bindings projected"
        );
        assert_eq!(
            view.provider_ids,
            ["provider"],
            "only workspace provider bindings projected"
        );
    }
}

#[test]
fn protocol_projection_rejects_cursor_scope_and_duplicate_members() {
    let mut data = source(CoordinationMode::Managed {
        repositories: Vec::new(),
    });
    data.as_of.workspace_id = "foreign".into();
    assert!(
        WorkspaceSnapshot::try_from(&data).is_err(),
        "foreign cursor cannot establish workspace metadata"
    );
    data.as_of.workspace_id = "workspace".into();
    data.memberships
        .push(data.memberships.first().expect("member").clone());
    assert!(
        WorkspaceSnapshot::try_from(&data).is_err(),
        "duplicate membership cannot silently overwrite a role"
    );
}

#[test]
fn read_error_mapping_preserves_revocation_and_capability_failures() {
    for (code, kind) in [
        (ErrorCode::Forbidden, WorkspaceErrorKind::Forbidden),
        (
            ErrorCode::Unauthenticated,
            WorkspaceErrorKind::Unauthenticated,
        ),
        (ErrorCode::NotFound, WorkspaceErrorKind::NotFound),
        (
            ErrorCode::UnsupportedOperation,
            WorkspaceErrorKind::Unsupported,
        ),
        (ErrorCode::RateLimited, WorkspaceErrorKind::Unavailable),
        (
            ErrorCode::CursorScopeMismatch,
            WorkspaceErrorKind::InvalidData,
        ),
    ] {
        let error = WorkspaceError::from(ApiError {
            code,
            message: "Safe message".into(),
            retry: RetryAdvice::Never,
        });
        assert_eq!(
            error.kind, kind,
            "read failures retain their recovery classification"
        );
        assert_eq!(
            error.to_string(),
            "Safe message",
            "safe provider message retained"
        );
    }
}
