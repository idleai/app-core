//! Portable session identities, metadata and participation records.

use serde::{Deserialize, Serialize};

use crate::workspace::{MemberInfo, WorkspaceMode};

use super::SessionInputUpdate;

/// One provider, authenticated audience and explicit workspace/chain binding.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct SessionContext {
    /// Configured coordination connection, not a credential.
    pub provider: String,
    /// Stable workspace identity.
    pub workspace_id: String,
    /// Authenticated audience; changing it retires all outstanding work.
    pub contributor_id: String,
    /// Engine-issued logical chain reference.
    pub chain: String,
    /// Standalone or managed adapter route.
    pub mode: WorkspaceMode,
}

/// Actual authenticated human or service, independent of resource ownership.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct SessionContributor {
    /// Stable contributor identity.
    pub contributor_id: String,
    /// Verified authentication namespace, without token material.
    pub issuer: String,
    /// Immutable subject at that issuer.
    pub subject: String,
}

/// Caller-allocated mutation identity, persisted by the host before dispatch.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct SessionMutationId {
    /// Unique across mutation kinds for this contributor/workspace.
    pub request_id: String,
    /// Authority-clock deadline for first receipt, in Unix milliseconds.
    pub expires_at_ms: u64,
}

/// Stable retry lookup, distinct from a client-local effect continuation ID.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct SessionRequestKey {
    /// Workspace containing the mutation.
    pub workspace_id: String,
    /// Original contributor, never the session owner by substitution.
    pub contributor_id: String,
    /// Original caller-generated mutation identity.
    pub request_id: String,
}

/// Immutable attribution and retry context supplied to the host.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct SessionRequest {
    /// Workspace scope.
    pub workspace_id: String,
    /// Identity returned by the authenticated snapshot adapter.
    pub contributor: SessionContributor,
    /// Original retry ID and first-receipt deadline.
    pub mutation: SessionMutationId,
}

impl SessionRequest {
    /// Return the coordination/runtime deduplication key.
    #[must_use]
    pub fn key(&self) -> SessionRequestKey {
        SessionRequestKey {
            workspace_id: self.workspace_id.clone(),
            contributor_id: self.contributor.contributor_id.clone(),
            request_id: self.mutation.request_id.clone(),
        }
    }
}

/// Authorized runtime route; relocation preserves the directory and logical item.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct SessionRuntimeBinding {
    /// Compute host executing this session; this is not a compute grant.
    pub host_id: String,
    /// Runtime authenticated as the input-state producer.
    pub runtime_id: String,
}

/// Explicit mapping to an `EditChain` logical session, never an observation ID.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct SessionItemBinding {
    /// Workspace's logical chain reference.
    pub chain: String,
    /// Full canonical logical session item ID allocated by the recording adapter.
    pub item: String,
}

/// Session purpose does not change input attribution.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum SessionKind {
    /// Human-directed or autonomous runner.
    Runner,
    /// Ambient Control session.
    Control,
}

/// Directory metadata paired explicitly with its runtime and logical history item.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct SessionInfo {
    /// Reconnectable coordination session identity.
    pub id: String,
    /// Owner who may manage sharing; never an implicit prompt author.
    pub owner: String,
    /// User-visible title.
    pub title: String,
    /// Runner or Control purpose.
    pub kind: SessionKind,
    /// Coordination metadata revision, not an input revision.
    pub revision: u64,
    /// Current authenticated runtime route.
    pub runtime: SessionRuntimeBinding,
    /// Optional parent directory session.
    pub parent: Option<String>,
    /// Explicit immutable logical session binding.
    pub history: SessionItemBinding,
}

/// Session participation only; no variant grants file, process or model access.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum SessionPermission {
    /// Read the authorized session.
    Observe,
    /// Submit an attributed prompt.
    SubmitInput,
    /// Manage participation, subject to provider policy.
    Invite,
}

/// Irreversible grant revocation, independent of temporary runtime availability.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum SessionGrantStatus {
    /// Issued, subject to current membership and expiry.
    Active,
    /// Withdrawn permanently; a new grant ID is needed to restore access.
    Revoked {
        /// Authority-clock revocation time.
        at_ms: u64,
        /// Contributor who revoked the grant.
        by: String,
    },
}

/// One independently revocable session invitation/participation grant.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct SessionGrant {
    /// Non-reusable grant identity.
    pub id: String,
    /// Shared coordination session.
    pub session_id: String,
    /// Invited contributor, independent of the owner.
    pub grantee: String,
    /// Original issuing contributor.
    pub granted_by: String,
    /// Explicit session actions; compute permissions are never projected here.
    pub permissions: Vec<SessionPermission>,
    /// Exclusive authority-clock expiry, when supplied.
    pub expires_at_ms: Option<u64>,
    /// Authority-assigned metadata revision.
    pub revision: u64,
    /// Active or revoked status.
    pub status: SessionGrantStatus,
}

/// Host capability availability; production adapters start unavailable.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum SessionCapability {
    /// No connected adapter currently provides this operation.
    #[default]
    Unavailable,
    /// The adapter can attempt this operation; it must still authorize each call.
    Available,
}

/// Separate runtime and coordination capabilities, supplied by the host.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct SessionCapabilities {
    /// Runtime session creation, including recording and directory registration.
    pub create: SessionCapability,
    /// Delivery of attributed input to a connected runtime.
    pub input: SessionCapability,
    /// Coordination participation mutations.
    pub share: SessionCapability,
}

/// Durable coordination cursor, separate from runtime input order and timestamps.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct SessionCursor {
    /// Workspace scope.
    pub workspace_id: String,
    /// Authorized audience.
    pub contributor_id: String,
    /// Provider-issued visibility generation.
    pub stream_id: String,
    /// Exclusive scanned-through position.
    pub position: u64,
}

/// Consistent session projection of an authorized f20 recovery snapshot.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct SessionSnapshot {
    /// Exact provider, audience and workspace binding of the request.
    pub context: SessionContext,
    /// Durable boundary for subsequent buffered/retained updates.
    pub cursor: SessionCursor,
    /// Verified submitting identity, resolved by the host connection.
    pub contributor: SessionContributor,
    /// Available adapters, never inferred from directory registration.
    pub capabilities: SessionCapabilities,
    /// Current active/revoked memberships needed to interpret participation.
    pub members: Vec<MemberInfo>,
    /// Authorized session metadata with explicit logical item bindings.
    pub sessions: Vec<SessionInfo>,
    /// Only session grants; compute/provider grants remain separate.
    pub grants: Vec<SessionGrant>,
    /// Last retained facts, authenticated by the provider when recorded, including
    /// prior runtimes after relocation. Missing prompt text remains unknown.
    pub inputs: Vec<SessionInputUpdate>,
    /// Provider's Unix clock for grant and first-receipt deadlines.
    pub now_ms: u64,
}
