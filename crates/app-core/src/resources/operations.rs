//! Correlated resource effects. Host adapters own authorization and execution.

use crux_core::capability::Operation;
use serde::{Deserialize, Serialize};

use super::{ModelKey, ModelPackage, ModelTarget, ResourceContext, ResourceSnapshot};

/// Host-persisted retry identity, independent of a Crux continuation token.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ResourceRequest {
    /// Unique for this contributor/workspace across mutation kinds.
    pub request_id: String,
    /// Original first-receipt deadline, in authority-clock Unix milliseconds.
    pub expires_at_ms: u64,
}

/// Immutable intent sent to an authenticated runtime adapter.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ResourceMutation {
    /// Connect to compute without granting file, process or session access.
    ConnectHost {
        /// Explicit compute binding in this workspace.
        host_id: String,
    },
    /// Ask the target runtime to authorize and adopt a provider-qualified model.
    SelectModel {
        /// Exact session/runtime/host, including the epoch for Control.
        target: ModelTarget,
        /// Published model to use.
        model: ModelKey,
    },
    /// Ask the runtime to install and serve one supported catalog package.
    InstallModel(ModelPackage),
}

/// Work performed by coordination or runtime adapters outside the reducer.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ResourceOperationKind {
    /// Authorized coordination discovery plus separately authenticated runtime facts.
    Snapshot,
    /// Execute once or retry unchanged using the original persisted identity.
    Mutate {
        /// Original retry identity and deadline.
        request: ResourceRequest,
        /// Exact immutable runtime intent.
        mutation: ResourceMutation,
    },
    /// Resolve an uncertain or running action without executing it again.
    Status(ResourceRequest),
}

/// Workspace/audience-scoped resource effect. Runtimes recheck grants and Control
/// ownership at execution; an enabled UI action is never authorization.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ResourceOperation {
    /// Exact authenticated adapter and workspace binding.
    pub context: ResourceContext,
    /// Discovery, runtime action or reconciliation.
    pub kind: ResourceOperationKind,
}

/// Safe resource failure classifications.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ResourceErrorCode {
    /// Malformed or mismatched host result.
    InvalidData,
    /// Client intent is absent from current discovery or permitted actions.
    InvalidSelection,
    /// Current authorization does not permit the operation.
    Forbidden,
    /// Authentication is missing or no longer valid.
    Unauthenticated,
    /// Runtime/provider cannot currently respond.
    Unavailable,
    /// Adapter does not support the operation or protocol.
    Unsupported,
    /// Revision, ownership, retry identity or current state conflicts.
    Conflict,
    /// First-receipt deadline or retained retry result expired.
    Expired,
}

/// Provider/runtime retry advice. Uncertainty never allocates a new identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ResourceRetryAdvice {
    /// No execution retry is permitted.
    Never,
    /// Retry the original identity, deadline and payload only.
    SameRequest {
        /// Earliest authority-clock retry time, if supplied.
        not_before_ms: Option<u64>,
    },
    /// Query the original action's status before deciding what happened.
    QueryStatus,
}

/// Safe operation error without credentials or provider diagnostics.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ResourceError {
    /// Stable classification.
    pub code: ResourceErrorCode,
    /// User-presentable explanation.
    pub message: String,
    /// Explicit recovery guidance from the responsible adapter.
    pub retry: ResourceRetryAdvice,
}

impl std::fmt::Display for ResourceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ResourceError {}

/// Reported execution stage. Only authenticated runtime facts can report success.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ResourceActionStage {
    /// Durable routing receipt only; neither runtime acceptance nor completion.
    Received,
    /// Runtime-confirmed work in progress, with a safe progress message.
    Running(String),
    /// Runtime confirmed completion; discovery is refreshed separately.
    Succeeded,
    /// Runtime confirmed a terminal failure.
    Failed(ResourceError),
}

/// Retained action fact, authenticated and correlated by the host adapter.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ResourceProgress {
    /// Exact workspace and authenticated audience of the original action.
    pub context: ResourceContext,
    /// Original immutable retry identity and deadline.
    pub request: ResourceRequest,
    /// Increasing action revision, independent of coordination positions.
    pub revision: u64,
    /// Last confirmed stage, preserved on replay.
    pub stage: ResourceActionStage,
}

/// Result of one resource effect.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ResourceResult {
    /// Authorized replacement; missing resources are removed from this scope.
    Snapshot(Box<ResourceSnapshot>),
    /// Durable receipt or authenticated runtime action fact.
    Progress(ResourceProgress),
    /// No retained result. This cannot establish that an action never executed.
    Unknown,
}

/// Typed Rust host result.
pub type ResourceOutput = Result<ResourceResult, ResourceError>;

/// Generated-shell result with the same binary layout as [`ResourceOutput`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ResourceResponse {
    /// Successful host response.
    Ok(ResourceResult),
    /// Safe host failure.
    Err(ResourceError),
}

impl Operation for ResourceOperation {
    type Output = ResourceOutput;
}
