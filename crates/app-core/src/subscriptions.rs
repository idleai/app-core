//! Shared join, reconnect and reconciliation state. Owner: f28/subscription-state.
//!
//! Hosts register change notifications before acknowledging a join. Every join
//! then reads a fresh replacement snapshot. Notifications must remain queued while
//! a read runs and between watch requests. Operation IDs and engine refresh counts
//! are never used as subscription checkpoints.

mod reducer;
mod types;

pub use idle_history::{
    connection::{Connection, ConnectionStatus, JoinState, PeerCheck, PeerProgress, peer_status},
    reconciliation::{ReconcileError, Revisions},
    requests::{RequestError, RequestTracker},
};
pub use reducer::{Model, SubscriptionEvent as Event, Subscriptions};
pub use types::*;

#[cfg(test)]
mod tests;
