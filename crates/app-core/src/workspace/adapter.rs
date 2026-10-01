//! Projection of the published coordination contracts into portable shell payloads.

use std::{error::Error, fmt};

use idle_protocol::v1::{
    Record,
    api::{ApiError, ErrorCode},
    events::RecoverySnapshot,
    membership::{MembershipStatus, Role},
    workspace::{CoordinationMode, Workspace as ProtocolWorkspace},
};

use super::{
    MemberInfo, MemberRole, MemberStatus, RepositoryInfo, WorkspaceError, WorkspaceErrorKind,
    WorkspaceInfo, WorkspaceMode, WorkspaceSnapshot, validation,
};

impl From<&Record<ProtocolWorkspace>> for WorkspaceInfo {
    fn from(record: &Record<ProtocolWorkspace>) -> Self {
        let workspace = &record.value;
        let (mode, repositories) = match &workspace.mode {
            CoordinationMode::Standalone { repository } => {
                (WorkspaceMode::Standalone, std::slice::from_ref(repository))
            }
            CoordinationMode::Managed { repositories } => {
                (WorkspaceMode::Managed, repositories.as_slice())
            }
        };
        Self {
            id: workspace.id.0.clone(),
            name: workspace.name.clone(),
            chain: workspace.chain.0.clone(),
            revision: record.revision.0,
            mode,
            repositories: repositories
                .iter()
                .map(|repository| RepositoryInfo {
                    id: repository.id.0.clone(),
                    name: repository.name.clone(),
                    remote: repository.remote.clone(),
                })
                .collect(),
        }
    }
}

impl TryFrom<&RecoverySnapshot> for WorkspaceSnapshot {
    type Error = WorkspaceError;

    fn try_from(snapshot: &RecoverySnapshot) -> Result<Self, Self::Error> {
        if snapshot.as_of.workspace_id != snapshot.workspace.value.id {
            return Err(validation::invalid(
                "Snapshot cursor and workspace scopes differ",
            ));
        }
        validation::unique_ids(
            snapshot
                .contributors
                .iter()
                .map(|entry| entry.value.id.0.as_str()),
        )?;
        let view = Self {
            workspace: WorkspaceInfo::from(&snapshot.workspace),
            members: snapshot
                .memberships
                .iter()
                .map(|record| {
                    let member = &record.value;
                    MemberInfo {
                        contributor_id: member.contributor_id.0.clone(),
                        display_name: snapshot
                            .contributors
                            .iter()
                            .find(|entry| entry.value.id == member.contributor_id)
                            .map_or_else(
                                || member.contributor_id.0.clone(),
                                |entry| entry.value.display_name.clone(),
                            ),
                        revision: record.revision.0,
                        role: match member.role {
                            Role::Owner => MemberRole::Owner,
                            Role::Admin => MemberRole::Admin,
                            Role::Member => MemberRole::Member,
                            Role::Viewer => MemberRole::Viewer,
                        },
                        status: match member.status {
                            MembershipStatus::Active => MemberStatus::Active,
                            MembershipStatus::Revoked => MemberStatus::Revoked,
                        },
                    }
                })
                .collect(),
            host_ids: snapshot
                .hosts
                .iter()
                .map(|entry| entry.value.id.0.clone())
                .collect(),
            provider_ids: snapshot
                .providers
                .iter()
                .map(|entry| entry.value.id.0.clone())
                .collect(),
        };
        validation::snapshot(&view)?;
        Ok(view)
    }
}

impl From<ApiError> for WorkspaceError {
    fn from(error: ApiError) -> Self {
        let kind = match error.code {
            ErrorCode::Unauthenticated => WorkspaceErrorKind::Unauthenticated,
            ErrorCode::Forbidden => WorkspaceErrorKind::Forbidden,
            ErrorCode::NotFound => WorkspaceErrorKind::NotFound,
            ErrorCode::UnsupportedOperation | ErrorCode::UnsupportedVersion => {
                WorkspaceErrorKind::Unsupported
            }
            ErrorCode::Unavailable | ErrorCode::RateLimited | ErrorCode::RequestExpired => {
                WorkspaceErrorKind::Unavailable
            }
            ErrorCode::InvalidRequest
            | ErrorCode::Conflict
            | ErrorCode::StaleRevision
            | ErrorCode::StaleControl
            | ErrorCode::IdempotencyConflict
            | ErrorCode::CursorScopeMismatch => WorkspaceErrorKind::InvalidData,
        };
        Self {
            kind,
            message: error.message,
        }
    }
}

impl fmt::Display for WorkspaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for WorkspaceError {}
