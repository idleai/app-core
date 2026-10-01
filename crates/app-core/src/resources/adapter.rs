//! f20 discovery projection without platform credentials or backend policy.

use idle_protocol::v1::{
    Record,
    api::{ApiError, ErrorCode, RetryAdvice},
    events::RecoverySnapshot,
    grants::{ComputePermission, GrantScope, GrantStatus, ProviderPermission},
    resources::{
        Availability, ComputeHost, Health, HostCapability, Model, ModelCapability, ModelProvider,
        ProviderKind,
    },
};

use crate::workspace::{MemberStatus, WorkspaceSnapshot};

use super::{
    ComputeFeature, ComputeHostInfo, ControllerLease, ControllerOwnership, ModelFeature, ModelKey,
    ModelProviderInfo, ModelProviderKind, ModelTarget, ResourceAvailability, ResourceContext,
    ResourceError, ResourceErrorCode, ResourceGrant, ResourceHealth, ResourcePermission,
    ResourceRetryAdvice, ResourceRuntimeInfo, ResourceScope, ResourceSnapshot, ServedModelInfo,
    validation,
};

/// Host-supplied context for an authenticated coordination snapshot. Runtime
/// capabilities must come from connected adapters, never directory presence.
#[derive(Clone, Debug)]
pub struct ResourceAdapterContext {
    /// Authorized connection and workspace binding.
    pub context: ResourceContext,
    /// Provider-aligned current time, used for freshness/expiry checks.
    pub now_ms: u64,
    /// Independently authenticated runtime facts; default is unavailable.
    pub runtime: ResourceRuntimeInfo,
}

impl From<&Health> for ResourceHealth {
    fn from(value: &Health) -> Self {
        Self {
            availability: match value.availability {
                Availability::Unknown => ResourceAvailability::Unknown,
                Availability::Available => ResourceAvailability::Available,
                Availability::Unavailable => ResourceAvailability::Unavailable,
            },
            observed_at_ms: value.observed_at.0,
            valid_until_ms: value.valid_until.0,
        }
    }
}

impl From<&Record<ComputeHost>> for ComputeHostInfo {
    fn from(value: &Record<ComputeHost>) -> Self {
        Self {
            id: value.value.id.0.clone(),
            owner: value.value.owner.0.clone(),
            name: value.value.name.clone(),
            revision: value.revision.0,
            features: value
                .value
                .capabilities
                .iter()
                .map(|feature| match feature {
                    HostCapability::Sessions => ComputeFeature::Sessions,
                    HostCapability::Files => ComputeFeature::Files,
                    HostCapability::Processes => ComputeFeature::Processes,
                    HostCapability::LocalModels => ComputeFeature::LocalModels,
                })
                .collect(),
            health: (&value.value.health).into(),
        }
    }
}

impl From<&Record<ModelProvider>> for ModelProviderInfo {
    fn from(value: &Record<ModelProvider>) -> Self {
        Self {
            id: value.value.id.0.clone(),
            owner: value.value.owner.0.clone(),
            name: value.value.name.clone(),
            revision: value.revision.0,
            kind: match &value.value.kind {
                ProviderKind::External => ModelProviderKind::External,
                ProviderKind::Local {
                    host_id,
                    runtime_id,
                } => ModelProviderKind::Local {
                    host_id: host_id.0.clone(),
                    runtime_id: runtime_id.0.clone(),
                },
            },
            health: (&value.value.health).into(),
        }
    }
}

impl From<&Record<Model>> for ServedModelInfo {
    fn from(value: &Record<Model>) -> Self {
        Self {
            key: ModelKey {
                provider_id: value.value.provider_id.0.clone(),
                model_id: value.value.id.0.clone(),
            },
            name: value.value.name.clone(),
            revision: value.revision.0,
            features: value
                .value
                .capabilities
                .iter()
                .map(|feature| match feature {
                    ModelCapability::Text => ModelFeature::Text,
                    ModelCapability::Tools => ModelFeature::Tools,
                    ModelCapability::Images => ModelFeature::Images,
                })
                .collect(),
            health: (&value.value.health).into(),
        }
    }
}

impl From<&idle_protocol::v1::control::ControlOwnership> for ControllerOwnership {
    fn from(value: &idle_protocol::v1::control::ControlOwnership) -> Self {
        Self {
            last_epoch: value.last_epoch.0,
            lease: value.lease.as_ref().map(|lease| ControllerLease {
                target: ModelTarget {
                    session_id: lease.fence.holder.session_id.0.clone(),
                    host_id: lease.fence.holder.host_id.0.clone(),
                    runtime_id: lease.fence.holder.runtime_id.0.clone(),
                    control_epoch: Some(lease.fence.epoch.0),
                },
                acquired_at_ms: lease.acquired_at.0,
                expires_at_ms: lease.expires_at.0,
            }),
        }
    }
}

impl ResourceSnapshot {
    /// Project authorized f20 resources and independent runtime observations.
    /// Only compute/provider grants for this audience are included. Session grants,
    /// resource ownership and workspace roles cannot enable resource actions.
    ///
    /// # Errors
    /// Rejects mismatched context/cursors, invalid ownership or runtime targets,
    /// duplicate identities and malformed resource records.
    pub fn from_protocol(
        value: &RecoverySnapshot,
        adapter: &ResourceAdapterContext,
    ) -> Result<Self, ResourceError> {
        let workspace = WorkspaceSnapshot::try_from(value)
            .map_err(|error| validation::invalid(&error.message))?;
        let context = &adapter.context;
        if workspace.workspace.id != context.workspace_id
            || workspace.workspace.chain != context.chain
            || workspace.workspace.mode != context.mode
            || value.as_of.contributor_id.0 != context.contributor_id
            || value.control.workspace_id.0 != context.workspace_id
            || value
                .control
                .lease
                .as_ref()
                .is_some_and(|lease| lease.fence.workspace_id.0 != context.workspace_id)
        {
            return Err(validation::invalid(
                "Resource snapshot scope does not match its connection",
            ));
        }
        for selection in &adapter.runtime.selections {
            let target = &selection.target;
            if !value.sessions.iter().any(|record| {
                let session = &record.value;
                session.id.0 == target.session_id
                    && session.runtime.host_id.0 == target.host_id
                    && session.runtime.runtime_id.0 == target.runtime_id
                    && matches!(
                        session.kind,
                        idle_protocol::v1::sessions::SessionKind::Control
                    ) == target.control_epoch.is_some()
            }) {
                return Err(validation::invalid(
                    "Model selection has no matching runtime session binding",
                ));
            }
        }
        let snapshot = Self {
            context: context.clone(),
            stream_id: value.as_of.stream_id.0.clone(),
            position: value.as_of.position.0,
            now_ms: adapter.now_ms,
            member_active: workspace.members.iter().any(|member| {
                member.contributor_id == context.contributor_id
                    && member.status == MemberStatus::Active
            }),
            hosts: value.hosts.iter().map(Into::into).collect(),
            providers: value.providers.iter().map(Into::into).collect(),
            models: value.models.iter().map(Into::into).collect(),
            grants: value
                .grants
                .iter()
                .filter(|record| record.value.grantee.0 == context.contributor_id)
                .filter_map(resource_grant)
                .collect(),
            controller: (&value.control).into(),
            runtime: adapter.runtime.clone(),
        };
        validation::snapshot(&snapshot, context, None)?;
        Ok(snapshot)
    }
}

fn resource_grant(record: &Record<idle_protocol::v1::grants::Grant>) -> Option<ResourceGrant> {
    let grant = &record.value;
    let (scope, permissions) = match &grant.scope {
        GrantScope::Session { .. } => return None,
        GrantScope::Compute {
            host_id,
            permissions,
        } => (
            ResourceScope::Host(host_id.0.clone()),
            permissions
                .iter()
                .filter_map(|permission| match permission {
                    ComputePermission::Connect => Some(ResourcePermission::ConnectHost),
                    ComputePermission::ManageModels => Some(ResourcePermission::InstallModel),
                    ComputePermission::ReadFiles
                    | ComputePermission::WriteFiles
                    | ComputePermission::Execute => None,
                })
                .collect(),
        ),
        GrantScope::Provider {
            provider_id,
            permissions,
        } => (
            ResourceScope::Provider(provider_id.0.clone()),
            permissions
                .iter()
                .filter_map(|permission| match permission {
                    ProviderPermission::UseModels => Some(ResourcePermission::UseModels),
                    ProviderPermission::Manage => None,
                })
                .collect(),
        ),
    };
    Some(ResourceGrant {
        id: grant.id.0.clone(),
        revision: record.revision.0,
        scope,
        permissions,
        expires_at_ms: grant.expires_at.map(|time| time.0),
        active: matches!(grant.status, GrantStatus::Active),
    })
}

impl From<ApiError> for ResourceError {
    fn from(value: ApiError) -> Self {
        Self {
            code: match value.code {
                ErrorCode::InvalidRequest | ErrorCode::CursorScopeMismatch => {
                    ResourceErrorCode::InvalidData
                }
                ErrorCode::UnsupportedVersion | ErrorCode::UnsupportedOperation => {
                    ResourceErrorCode::Unsupported
                }
                ErrorCode::Unauthenticated => ResourceErrorCode::Unauthenticated,
                ErrorCode::Forbidden => ResourceErrorCode::Forbidden,
                ErrorCode::NotFound => ResourceErrorCode::InvalidSelection,
                ErrorCode::Conflict
                | ErrorCode::StaleRevision
                | ErrorCode::StaleControl
                | ErrorCode::IdempotencyConflict => ResourceErrorCode::Conflict,
                ErrorCode::RequestExpired => ResourceErrorCode::Expired,
                ErrorCode::Unavailable | ErrorCode::RateLimited => ResourceErrorCode::Unavailable,
            },
            message: value.message,
            retry: match value.retry {
                RetryAdvice::Never => ResourceRetryAdvice::Never,
                RetryAdvice::SameRequest { not_before } => ResourceRetryAdvice::SameRequest {
                    not_before_ms: not_before.map(|time| time.0),
                },
                RetryAdvice::QueryStatus => ResourceRetryAdvice::QueryStatus,
            },
        }
    }
}
