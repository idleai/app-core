//! Attributed runtime facts and independent coordination acknowledgements.

use serde::{Deserialize, Serialize};

use super::{SessionContributor, SessionCursor, SessionError, SessionRequestKey};

/// Stable identity of one contributor's input in a coordination session.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct SessionInputRef {
    /// Target coordination session, not its logical history item.
    pub session_id: String,
    /// Original retry scope and contributor.
    pub request: SessionRequestKey,
}

/// Durable backend receipt; it contains no runtime order or execution status.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct SessionReceipt {
    /// Unchanged mutation identity.
    pub request: SessionRequestKey,
    /// Original authority-clock receipt time.
    pub received_at_ms: u64,
    /// Inclusive minimum retention of the original result.
    pub retry_until_ms: u64,
}

/// Runtime-authored acceptance and order; order never comes from a local queue.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct SessionDelivery {
    /// Runtime's durable acceptance time.
    pub accepted_at_ms: u64,
    /// Positive session-wide delivery order, stable across runtime relocation.
    pub order: u64,
    /// Runtime's durable order-assignment time.
    pub ordered_at_ms: u64,
}

/// Terminal runtime result, independent of transport delivery success.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum SessionCompletion {
    /// Runtime confirmed successful processing.
    Succeeded,
    /// Runtime confirmed failure, possibly after partial processing.
    Failed(SessionError),
    /// Runtime confirmed cancellation, possibly after partial processing.
    Cancelled,
}

/// Full runtime-confirmed input state; observers may skip intermediate revisions.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum SessionInputState {
    /// Runtime refused the input before acceptance.
    Rejected {
        /// Runtime rejection time.
        at_ms: u64,
        /// Runtime's explicit reason.
        error: SessionError,
    },
    /// Runtime durably accepted the input, with order still unknown.
    Accepted {
        /// Runtime acceptance time.
        at_ms: u64,
    },
    /// Runtime assigned delivery order, without claiming execution started.
    Ordered(SessionDelivery),
    /// Runtime confirmed processing started.
    Running {
        /// Original stable acceptance/order facts.
        delivery: SessionDelivery,
        /// Runtime start time.
        started_at_ms: u64,
    },
    /// Runtime confirmed a terminal outcome.
    Completed {
        /// Original stable acceptance/order facts.
        delivery: SessionDelivery,
        /// Runtime completion time.
        completed_at_ms: u64,
        /// Explicit result, including failure/cancellation.
        outcome: SessionCompletion,
    },
}

impl SessionInputState {
    /// Return delivery facts only when the runtime has assigned an order.
    #[must_use]
    pub const fn delivery(&self) -> Option<&SessionDelivery> {
        match self {
            Self::Rejected { .. } | Self::Accepted { .. } => None,
            Self::Ordered(delivery)
            | Self::Running { delivery, .. }
            | Self::Completed { delivery, .. } => Some(delivery),
        }
    }
}

/// Authenticated runtime fact projected from f20's `RuntimeInputUpdate`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct SessionInputUpdate {
    /// Original input and retry identity.
    pub input: SessionInputRef,
    /// Verified original contributor, matching the input's request key.
    pub contributor: SessionContributor,
    /// Authenticated producer matching the current session runtime binding.
    pub runtime_id: String,
    /// Positive monotonic input revision; unrelated to event cursor positions.
    pub revision: u64,
    /// Full runtime state at this revision.
    pub state: SessionInputState,
}

/// Coordination acknowledgement only; neither variant confirms execution.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum SessionAcknowledgement {
    /// Prompt durably received for forwarding.
    Received(SessionReceipt),
    /// Metadata committed; read through the cursor before presenting new grants.
    Committed {
        /// Durable mutation receipt.
        receipt: SessionReceipt,
        /// Authority-clock commit time.
        committed_at_ms: u64,
        /// Commit boundary, never advanced locally by this acknowledgement alone.
        through: SessionCursor,
    },
}
