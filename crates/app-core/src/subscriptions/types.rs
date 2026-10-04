//! Portable subscription effects and views for native and WASM hosts.

use crux_core::capability::Operation;
use serde::{Deserialize, Serialize};

use super::ConnectionStatus;

pub use idle_history::binding::Context;

/// Host work for one explicit subscription context.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct SubscriptionOperation {
    /// Provider and audience authorized for this work.
    pub context: Context,
    /// Requested platform work.
    pub action: SubscriptionAction,
}

/// Transport and timers remain host responsibilities; the core owns their order.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum SubscriptionAction {
    /// Open authorized delivery and start buffering invalidations before returning.
    Join,
    /// Await a buffered or future invalidation. Exactly one watch is pending.
    Watch {
        /// Opaque connection handle, never a bearer credential or resume cursor.
        connection: String,
    },
    /// Wait before another join; the core discards timers from retired contexts.
    Wait {
        /// Host timer duration in milliseconds.
        delay_ms: u32,
    },
    /// Release only this connection and its queued notifications.
    Leave {
        /// Opaque connection handle allocated by the matching join.
        connection: String,
    },
}

/// Results are admitted only through the matching host continuation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum SubscriptionResult {
    /// Notification buffering is active; a replacement snapshot is now required.
    Joined {
        /// Handle of this connection, not a durable event position.
        connection: String,
    },
    /// Stored records, conflicts or content may have changed; reconcile the view.
    Changed,
    /// The provider lost delivery continuity; rejoin and read a fresh snapshot.
    Closed,
    /// Retry timer elapsed.
    Elapsed,
    /// The host released the connection.
    Left,
}

/// Host failures distinguish retryable transport loss from unavailable access.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum SubscriptionErrorKind {
    /// The host may retry after a bounded delay.
    Transport,
    /// A provider or required host capability is not installed.
    Unavailable,
    /// Current authorization no longer permits this context.
    Unauthorized,
}

/// Presentable subscription failure without credentials or transport secrets.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct SubscriptionError {
    /// Recovery behavior supplied by the responsible provider.
    pub kind: SubscriptionErrorKind,
    /// User-visible explanation.
    pub message: String,
}

/// Result returned by typed hosts.
pub type SubscriptionOutput = Result<SubscriptionResult, SubscriptionError>;

/// Generated shell response with the same binary layout as [`SubscriptionOutput`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum SubscriptionResponse {
    /// Successful host response.
    Ok(SubscriptionResult),
    /// Presentable failure.
    Err(SubscriptionError),
}

impl Operation for SubscriptionOperation {
    type Output = SubscriptionOutput;
}

/// Client-visible connection and recovery state, independent of presentation.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct SubscriptionViewModel {
    /// Current provider, audience and chain.
    pub context: Option<Context>,
    /// A connection becomes live only after its replacement read succeeds.
    pub status: ConnectionStatus,
    /// Last presentable failure, cleared by successful reconciliation.
    pub error: Option<SubscriptionError>,
}
