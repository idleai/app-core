//! Session selection, mutation progress and attributed prompt presentation.

use serde::{Deserialize, Serialize};

use super::{
    SessionAcknowledgement, SessionCapabilities, SessionContext, SessionContributor, SessionError,
    SessionGrant, SessionInfo, SessionInputRef, SessionInputUpdate, SessionMutation,
    SessionPermission, SessionRequest,
};

/// Loading/connection state independent of retained session data.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum SessionLoadState {
    /// No operation requested.
    #[default]
    Idle,
    /// Awaiting a host snapshot or buffered changes.
    Loading,
    /// Last operation succeeded.
    Ready,
    /// Explicit failure; retained data is stale until recovery.
    Failed(SessionError),
}

/// Relationship to the signed-in contributor, separate from prompt authorship.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum SessionRelationship {
    /// The current contributor owns this directory session.
    Owned,
    /// The current contributor has an active participation grant.
    Invited,
}

/// Authorized visible session and its effective participation actions.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct SessionView {
    /// Directory and explicit runtime/history identities.
    pub session: SessionInfo,
    /// Owned versus invited status for this client.
    pub relationship: SessionRelationship,
    /// Currently available session actions; still authorized by the host on use.
    pub actions: Vec<SessionPermission>,
    /// Visible participation grants, including expired and revoked records.
    pub grants: Vec<SessionGrant>,
}

/// Mutation progress does not substitute for runtime input status.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum SessionMutationState {
    /// Host work is in flight.
    Pending,
    /// Coordination durably received input or committed metadata.
    Acknowledged(SessionAcknowledgement),
    /// Runtime allocation and directory registration completed.
    Created(SessionInfo),
    /// A direct runtime submission returned a fact; inspect the prompt's state.
    RuntimeReported,
    /// Operation failed; the retry advice controls safe recovery.
    Failed(SessionError),
    /// Coordination no longer knows the result; execution may still have happened.
    Unknown,
}

/// Reviewable creation, invitation, revocation or input operation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct SessionMutationView {
    /// Immutable original attribution and retry identity.
    pub request: SessionRequest,
    /// Exact original intent, retained for safe retries.
    pub mutation: SessionMutation,
    /// Pending/error/acknowledgement feedback.
    pub state: SessionMutationState,
}

/// Attributed prompt, including local pending input and recovered remote facts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct SessionPromptView {
    /// Stable session/contributor/request identity.
    pub input: SessionInputRef,
    /// Actual contributor, never replaced with the owner.
    pub contributor: SessionContributor,
    /// Exact locally submitted text; absent when f20 recovery has no text.
    pub text: Option<String>,
    /// Local dispatch feedback, absent for another client's submission.
    pub submission: Option<SessionMutationState>,
    /// Runtime-confirmed state only; absent even after backend receipt.
    pub runtime: Option<SessionInputUpdate>,
}

/// Portable session state for all client surfaces.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct SessionViewModel {
    /// Current provider, audience and chain.
    pub context: Option<SessionContext>,
    /// Snapshot progress and staleness.
    pub load: SessionLoadState,
    /// Retained-change delivery status, separate from runtime execution.
    pub updates: SessionLoadState,
    /// Host capabilities; unavailable until a connected adapter reports them.
    pub capabilities: SessionCapabilities,
    /// Owned and actively invited sessions only.
    pub sessions: Vec<SessionView>,
    /// Selected coordination session, preserved through same-context recovery.
    pub selected: Option<String>,
    /// Exact logical session binding for history consumers, when selected.
    pub selected_history: Option<super::SessionItemBinding>,
    /// Attributed visible inputs. Vector position is not runtime delivery order.
    pub prompts: Vec<SessionPromptView>,
    /// Local pending, failed and acknowledged actions.
    pub mutations: Vec<SessionMutationView>,
    /// Client validation error without replacing a valid selection.
    pub action_error: Option<SessionError>,
}
