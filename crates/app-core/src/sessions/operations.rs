//! Typed runtime/coordination operations and their portable host results.

use crux_core::capability::Operation;
use serde::{Deserialize, Serialize};

use crate::workspace::MemberInfo;

use super::{
    SessionAcknowledgement, SessionContext, SessionCursor, SessionGrant, SessionInfo,
    SessionInputRef, SessionInputUpdate, SessionPermission, SessionRequest, SessionRequestKey,
    SessionSnapshot,
};

/// New runner intent. The runtime allocates session, runtime and logical item IDs.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct SessionDraft {
    /// Proposed user-visible title.
    pub title: String,
    /// Selected host; creation still requires independent compute authorization.
    pub host_id: String,
    /// Optional existing parent directory session.
    pub parent: Option<String>,
}

/// Mutations retain their original payload, attribution and deadline on retries.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum SessionMutation {
    /// Allocate a runtime runner, bind its history item and register its directory.
    /// A directory-only `PutSession` commit cannot resolve this as `Created`.
    Create(SessionDraft),
    /// Route exact text using f20's attributed `SubmitInput` envelope.
    Submit {
        /// Existing coordination session.
        session_id: String,
        /// Exact text, including whitespace retained across retries.
        text: String,
    },
    /// Issue a session-only participation grant through coordination.
    Invite {
        /// Session being shared.
        session_id: String,
        /// New non-reusable grant identity.
        grant_id: String,
        /// Invited contributor.
        grantee: String,
        /// Explicit participation permissions.
        permissions: Vec<SessionPermission>,
        /// Optional exclusive grant deadline.
        expires_at_ms: Option<u64>,
    },
    /// Revoke a participation grant without changing ownership or compute access.
    Revoke {
        /// Session whose grant is being revoked.
        session_id: String,
        /// Existing grant identity.
        grant_id: String,
        /// Current metadata revision, checked atomically by the provider.
        expected_revision: u64,
    },
}

impl SessionMutation {
    pub(super) fn session_id(&self) -> Option<&str> {
        match self {
            Self::Create(_) => None,
            Self::Submit { session_id, .. }
            | Self::Invite { session_id, .. }
            | Self::Revoke { session_id, .. } => Some(session_id),
        }
    }
}

/// Host operation for one authenticated session context.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct SessionOperation {
    /// Exact provider, audience and logical chain binding.
    pub context: SessionContext,
    /// Typed work to execute outside the reducer.
    pub action: SessionAction,
}

/// Runtime and coordination work remain distinct at the host boundary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum SessionAction {
    /// Read an authorized consistent snapshot and explicit logical session bindings.
    Snapshot,
    /// Await buffered or retained changes after a durable cursor; never busy-poll.
    Watch {
        /// Exclusive resume cursor, including its visibility generation.
        after: SessionCursor,
    },
    /// Execute a retry-safe human action. The host verifies its attribution.
    Mutate {
        /// Original authenticated contributor and retry identity.
        request: SessionRequest,
        /// Immutable semantic payload.
        mutation: SessionMutation,
    },
    /// Reconcile an uncertain coordination outcome by its original key.
    RequestStatus(SessionRequestKey),
    /// Read the latest runtime fact without resubmitting input.
    InputStatus(SessionInputRef),
}

/// Relevant full-state changes projected from the f20 retained event stream.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum SessionChange {
    /// New directory metadata, paired with its explicit history binding.
    Session(SessionInfo),
    /// Participation grant issued, updated or revoked.
    Grant(SessionGrant),
    /// Membership changed; revocation overrides session grants immediately.
    Member(MemberInfo),
    /// Runtime-confirmed attributed input state.
    Input(SessionInputUpdate),
}

/// One retained session-related event at a coordination position.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct SessionChangeEvent {
    /// Durable event position; not prompt order.
    pub position: u64,
    /// Full new state, validated before advancing the cursor.
    pub change: SessionChange,
}

/// Atomic projected catch-up page, including a scanned-through boundary.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct SessionChanges {
    /// Echo of the requested exclusive cursor.
    pub after: SessionCursor,
    /// Scanned-through boundary, possibly beyond the last relevant event.
    pub through: SessionCursor,
    /// Strictly increasing relevant events; unrelated f20 records may be filtered.
    pub events: Vec<SessionChangeEvent>,
    /// Current provider clock for expiry checks.
    pub now_ms: u64,
}

/// Host results are admitted only through a matching pending continuation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum SessionResult {
    /// Consistent session directory and last retained runtime facts.
    Snapshot(Box<SessionSnapshot>),
    /// Relevant retained changes from the authenticated provider.
    Changes(SessionChanges),
    /// Cursor expired or audience/provider generation changed; fetch a snapshot.
    SnapshotRequired,
    /// Runtime confirmed allocation and the host completed directory registration.
    /// This establishes neither prompt acceptance nor execution.
    Created {
        /// Exact original create request key.
        request: SessionRequestKey,
        /// Runtime/coordination identity with its explicit logical session item.
        session: SessionInfo,
    },
    /// Backend receipt/commit without runtime success claims.
    Acknowledged(SessionAcknowledgement),
    /// Latest authenticated runtime input fact from an input-status query.
    Input(SessionInputUpdate),
    /// No retained coordination result; this does not authorize a new mutation.
    Unknown,
}

/// Portable f20 failure classifications plus client-side invalid selections.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum SessionErrorCode {
    /// Invalid payload or identity binding.
    InvalidRequest,
    /// Unsupported protocol version.
    UnsupportedVersion,
    /// Adapter does not implement the operation.
    UnsupportedOperation,
    /// Missing or invalid authenticated connection.
    Unauthenticated,
    /// Current policy or participation forbids the action.
    Forbidden,
    /// Resource absent or no longer visible.
    NotFound,
    /// Incompatible current entity state.
    Conflict,
    /// Optimistic metadata precondition failed.
    StaleRevision,
    /// Control fencing is no longer current.
    StaleControl,
    /// Retry key reused with changed semantic content.
    IdempotencyConflict,
    /// First-receipt deadline or retention elapsed.
    RequestExpired,
    /// Recovery scope does not match.
    CursorScopeMismatch,
    /// Temporary runtime/provider unavailability.
    Unavailable,
    /// Provider requires waiting before retry.
    RateLimited,
    /// Client action has no valid current target.
    InvalidSelection,
}

/// Retry advice preserved from f20; uncertain outcomes require lookup first.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum SessionRetryAdvice {
    /// No automatic retry is permitted.
    Never,
    /// Retry unchanged after the supplied authority-clock deadline.
    SameRequest {
        /// Earliest retry time, if present.
        not_before_ms: Option<u64>,
    },
    /// Reconcile the original key before deciding what happened.
    QueryStatus,
}

/// Presentable operation failure; not a runtime rejection or terminal input fact.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct SessionError {
    /// Stable failure classification.
    pub code: SessionErrorCode,
    /// Safe explanation, without credentials or private diagnostics.
    pub message: String,
    /// Explicit retry/recovery advice.
    pub retry: SessionRetryAdvice,
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for SessionError {}

/// Typed Rust host output.
pub type SessionOutput = Result<SessionResult, SessionError>;

/// Named generated-shell result with the binary layout of [`SessionOutput`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum SessionResponse {
    /// Successful host result.
    Ok(Box<SessionResult>),
    /// Provider/runtime operation failure.
    Err(SessionError),
}

impl Operation for SessionOperation {
    type Output = SessionOutput;
}
