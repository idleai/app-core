//! Controller ownership is distinct from a current runtime observation.

use serde::{Deserialize, Serialize};

use super::{ModelTarget, ResourceAvailability, ResourceHealth};

/// Durable controller ownership, including its retained epoch watermark.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ControllerOwnership {
    /// Highest authority-issued epoch, retained after release.
    pub last_epoch: u64,
    /// At most one assigned controller; expiry cannot elect a successor.
    pub lease: Option<ControllerLease>,
}

/// Authority-issued controller lease. Presentation never authorizes execution.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ControllerLease {
    /// Exact runtime holder and nonzero ownership epoch.
    pub target: ModelTarget,
    /// Authority-clock acquisition time.
    pub acquired_at_ms: u64,
    /// Exclusive authority-clock deadline.
    pub expires_at_ms: u64,
}

/// Runtime-reported controller lifecycle; leases alone cannot establish it.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ControllerPhase {
    /// No current runtime fact is available.
    #[default]
    Unknown,
    /// Runtime is preparing the controller.
    Starting,
    /// Runtime reports active controller work.
    Running,
    /// Runtime reports an intentional pause.
    Paused,
    /// Runtime reports the controller stopped.
    Stopped,
    /// Safe runtime error message.
    Failed(String),
}

/// Authenticated observation belonging to one exact controller holder/epoch.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ControllerRuntime {
    /// Runtime identity and ownership epoch that produced this observation.
    pub target: ModelTarget,
    /// Runtime-observed lifecycle.
    pub phase: ControllerPhase,
    /// Observation freshness and runtime reachability.
    pub health: ResourceHealth,
}

/// Ownership status evaluated using the provider-aligned clock.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ControllerAssignment {
    /// Ownership has not been loaded or delivery continuity was lost.
    #[default]
    Unknown,
    /// Authority reports no current lease.
    Unassigned,
    /// An authority lease is within its validity window.
    Assigned,
    /// Last known lease expired; no replacement is inferred.
    Expired,
}

/// Shared controller display; ownership and runtime health remain separate.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ControllerView {
    /// Last authority-confirmed lease and epoch watermark.
    pub ownership: ControllerOwnership,
    /// Current interpretation of that lease.
    pub assignment: ControllerAssignment,
    /// Runtime reachability, independent of assignment.
    pub availability: ResourceAvailability,
    /// Current runtime phase, unknown after health/lease expiry.
    pub phase: ControllerPhase,
}
