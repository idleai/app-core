//! Explicit logical session mapping and projected durable recovery.

use std::collections::BTreeMap;

use idle_protocol::v1::{
    Record,
    events::{EventBody, EventPage, Recovery, RecoveryCursor, RecoverySnapshot},
    grants::{GrantScope, GrantStatus},
    identity::{EventPosition, SessionId},
    membership::{MembershipStatus, Role},
    sessions::{Session, SessionKind as ProtocolKind},
};

use crate::workspace::{MemberInfo, MemberRole, MemberStatus, WorkspaceInfo, WorkspaceSnapshot};

use super::{
    super::{
        SessionChange, SessionChangeEvent, SessionChanges, SessionCursor, SessionError,
        SessionGrant, SessionGrantStatus, SessionInfo, SessionItemBinding, SessionKind,
        SessionResult, SessionRuntimeBinding, SessionSnapshot, model::State, validation,
    },
    SessionAdapterContext,
};

impl From<&RecoveryCursor> for SessionCursor {
    fn from(value: &RecoveryCursor) -> Self {
        Self {
            workspace_id: value.workspace_id.0.clone(),
            contributor_id: value.contributor_id.0.clone(),
            stream_id: value.stream_id.0.clone(),
            position: value.position.0,
        }
    }
}

impl From<&SessionCursor> for RecoveryCursor {
    fn from(value: &SessionCursor) -> Self {
        Self {
            workspace_id: value.workspace_id.as_str().into(),
            contributor_id: value.contributor_id.as_str().into(),
            stream_id: value.stream_id.as_str().into(),
            position: EventPosition(value.position),
        }
    }
}

impl SessionInfo {
    /// Pair f20 directory metadata with a recording adapter's explicit item mapping.
    /// Directory IDs, runtime IDs and observation IDs are never converted into items.
    #[must_use]
    pub fn from_protocol(value: &Record<Session>, history: SessionItemBinding) -> Self {
        let session = &value.value;
        Self {
            id: session.id.0.clone(),
            owner: session.owner.0.clone(),
            title: session.title.clone(),
            kind: match session.kind {
                ProtocolKind::Runner => SessionKind::Runner,
                ProtocolKind::Control => SessionKind::Control,
            },
            revision: value.revision.0,
            runtime: SessionRuntimeBinding {
                host_id: session.runtime.host_id.0.clone(),
                runtime_id: session.runtime.runtime_id.0.clone(),
            },
            parent: session.parent.as_ref().map(|id| id.0.clone()),
            history,
        }
    }
}

fn bound_session(
    value: &Record<Session>,
    bindings: &BTreeMap<SessionId, SessionItemBinding>,
) -> Result<SessionInfo, SessionError> {
    let history = bindings.get(&value.value.id).ok_or_else(|| {
        validation::invalid(
            "Directory session is missing its explicit logical history item binding",
        )
    })?;
    Ok(SessionInfo::from_protocol(value, history.clone()))
}

fn session_grant(value: &Record<idle_protocol::v1::grants::Grant>) -> Option<SessionGrant> {
    let grant = &value.value;
    let GrantScope::Session {
        session_id,
        permissions,
    } = &grant.scope
    else {
        return None;
    };
    Some(SessionGrant {
        id: grant.id.0.clone(),
        session_id: session_id.0.clone(),
        grantee: grant.grantee.0.clone(),
        granted_by: grant.granted_by.0.clone(),
        permissions: permissions.iter().copied().map(Into::into).collect(),
        expires_at_ms: grant.expires_at.map(|expiry| expiry.0),
        revision: value.revision.0,
        status: match &grant.status {
            GrantStatus::Active => SessionGrantStatus::Active,
            GrantStatus::Revoked {
                revoked_at,
                revoked_by,
            } => SessionGrantStatus::Revoked {
                at_ms: revoked_at.0,
                by: revoked_by.0.clone(),
            },
        },
    })
}

impl SessionSnapshot {
    /// Project f20 metadata using authenticated host capabilities and explicit items.
    ///
    /// # Errors
    /// Rejects mismatched workspace/audience/chain/mode, missing item mappings or
    /// malformed directory records. No item identity is inferred from a session ID.
    pub fn from_protocol(
        value: &RecoverySnapshot,
        adapter: &SessionAdapterContext,
        bindings: &BTreeMap<SessionId, SessionItemBinding>,
    ) -> Result<Self, SessionError> {
        let workspace = WorkspaceSnapshot::try_from(value)
            .map_err(|error| validation::invalid(&error.message))?;
        if workspace.workspace.id != adapter.context.workspace_id
            || workspace.workspace.chain != adapter.context.chain
            || workspace.workspace.mode != adapter.context.mode
        {
            return Err(validation::invalid(
                "Session directory does not match the explicit workspace binding",
            ));
        }
        let snapshot = Self {
            context: adapter.context.clone(),
            cursor: (&value.as_of).into(),
            contributor: (&adapter.contributor).into(),
            capabilities: adapter.capabilities.clone(),
            members: workspace.members,
            sessions: value
                .sessions
                .iter()
                .map(|session| bound_session(session, bindings))
                .collect::<Result<_, _>>()?,
            grants: value.grants.iter().filter_map(session_grant).collect(),
            inputs: value.inputs.iter().map(Into::into).collect(),
            now_ms: adapter.now_ms,
        };
        validation::snapshot(
            &snapshot,
            &State {
                context: Some(adapter.context.clone()),
                ..State::default()
            },
        )?;
        Ok(snapshot)
    }
}

impl SessionResult {
    /// Project f20 recovery pages while preserving their full scanned boundary.
    ///
    /// # Errors
    /// Rejects cross-scope events, invalid cursor ordering, changed workspace bindings
    /// and session directory events without explicit logical item mappings.
    pub fn from_recovery(
        value: &Recovery,
        adapter: &SessionAdapterContext,
        bindings: &BTreeMap<SessionId, SessionItemBinding>,
    ) -> Result<Self, SessionError> {
        match value {
            Recovery::SnapshotRequired(_) => Ok(Self::SnapshotRequired),
            Recovery::Events(page) => project_page(page, adapter, bindings).map(Self::Changes),
        }
    }
}

fn project_page(
    page: &EventPage,
    adapter: &SessionAdapterContext,
    bindings: &BTreeMap<SessionId, SessionItemBinding>,
) -> Result<SessionChanges, SessionError> {
    let after: SessionCursor = (&page.after).into();
    let through: SessionCursor = (&page.through).into();
    validation::cursor(&after, &adapter.context)?;
    if !validation::same_stream(&after, &through) || through.position < after.position {
        return Err(validation::invalid("Invalid f20 recovery boundaries"));
    }
    let mut previous = after.position;
    let mut events = Vec::new();
    for event in &page.events {
        let cursor: SessionCursor = (&event.cursor).into();
        if !validation::same_stream(&after, &cursor)
            || cursor.position <= previous
            || cursor.position > through.position
        {
            return Err(validation::invalid("Invalid f20 event scope or order"));
        }
        previous = cursor.position;
        if let Some(change) = change(&event.body, adapter, bindings)? {
            events.push(SessionChangeEvent {
                position: cursor.position,
                change,
            });
        }
    }
    Ok(SessionChanges {
        after,
        through,
        events,
        now_ms: adapter.now_ms,
    })
}

fn change(
    body: &EventBody,
    adapter: &SessionAdapterContext,
    bindings: &BTreeMap<SessionId, SessionItemBinding>,
) -> Result<Option<SessionChange>, SessionError> {
    let result = match body {
        EventBody::SessionChanged(session) => {
            Some(SessionChange::Session(bound_session(session, bindings)?))
        }
        EventBody::GrantChanged(grant) => session_grant(grant).map(SessionChange::Grant),
        EventBody::InputUpdated(input) => Some(SessionChange::Input(input.into())),
        EventBody::MembershipChanged(member) => Some(SessionChange::Member(MemberInfo {
            contributor_id: member.value.contributor_id.0.clone(),
            display_name: member.value.contributor_id.0.clone(),
            revision: member.revision.0,
            role: match member.value.role {
                Role::Owner => MemberRole::Owner,
                Role::Admin => MemberRole::Admin,
                Role::Member => MemberRole::Member,
                Role::Viewer => MemberRole::Viewer,
            },
            status: match member.value.status {
                MembershipStatus::Active => MemberStatus::Active,
                MembershipStatus::Revoked => MemberStatus::Revoked,
            },
        })),
        EventBody::WorkspaceChanged(workspace) => {
            let info = WorkspaceInfo::from(workspace);
            if info.id != adapter.context.workspace_id
                || info.chain != adapter.context.chain
                || info.mode != adapter.context.mode
            {
                return Err(validation::invalid(
                    "Workspace routing changed; reconnect session context",
                ));
            }
            None
        }
        EventBody::ContributorChanged(_)
        | EventBody::InvitationChanged(_)
        | EventBody::HostChanged(_)
        | EventBody::ProviderChanged(_)
        | EventBody::ModelChanged(_)
        | EventBody::ResourceDetached { .. }
        | EventBody::ControlChanged(_) => None,
    };
    Ok(result)
}
