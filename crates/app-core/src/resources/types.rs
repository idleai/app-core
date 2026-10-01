//! Portable discovery records and explicit runtime capabilities.

use serde::{Deserialize, Serialize};

use super::{ControllerOwnership, ControllerRuntime};
use crate::workspace::WorkspaceMode;

/// Provider connection and authenticated audience for one workspace/chain binding.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ResourceContext {
    /// Configured coordination adapter, without credentials.
    pub provider: String,
    /// Workspace containing these resource bindings.
    pub workspace_id: String,
    /// Authenticated audience; changing it retires all previous responses.
    pub contributor_id: String,
    /// Explicit workspace logical chain.
    pub chain: String,
    /// Standalone or managed coordination route.
    pub mode: WorkspaceMode,
}

/// Reachability is independent of authorization.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ResourceAvailability {
    /// No current observation, including expired health records.
    #[default]
    Unknown,
    /// Currently reported healthy.
    Available,
    /// Reported unreachable; the resource identity remains intact.
    Unavailable,
}

/// Runtime/provider health with an exclusive freshness deadline.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ResourceHealth {
    /// Last reported reachability.
    pub availability: ResourceAvailability,
    /// Producer observation time in Unix milliseconds.
    pub observed_at_ms: u64,
    /// Exclusive authority-aligned freshness deadline.
    pub valid_until_ms: u64,
}

impl ResourceHealth {
    /// Expired or future observations cannot establish current availability.
    #[must_use]
    pub const fn at(&self, now_ms: u64) -> ResourceAvailability {
        if now_ms < self.observed_at_ms || now_ms >= self.valid_until_ms {
            ResourceAvailability::Unknown
        } else {
            self.availability
        }
    }
}

/// Connected adapter support, never inferred from directory registration.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ResourceCapability {
    /// No adapter can currently attempt this action.
    #[default]
    Unavailable,
    /// An adapter can attempt it, subject to current authorization.
    Available,
}

/// Actual runtime adapters connected by the host. All default to unavailable.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ResourceCapabilities {
    /// Authorized compute connection adapter.
    pub connect_host: ResourceCapability,
    /// Runtime-confirmed model selection adapter.
    pub select_model: ResourceCapability,
    /// Local model installation and serving adapter.
    pub install_model: ResourceCapability,
}

/// Advertised compute features, still subject to grants and runtime policy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ComputeFeature {
    /// Agent session execution.
    Sessions,
    /// Remote file operations.
    Files,
    /// Remote process operations.
    Processes,
    /// Model installation and serving.
    LocalModels,
}

/// Host publication in the enclosing workspace, not global ownership state.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ComputeHostInfo {
    /// Reusable compute identity.
    pub id: String,
    /// Actual owner, distinct from this client's contributor.
    pub owner: String,
    /// Display label.
    pub name: String,
    /// Workspace publication revision.
    pub revision: u64,
    /// Runtime-advertised features.
    pub features: Vec<ComputeFeature>,
    /// Last reported health.
    pub health: ResourceHealth,
}

/// Model provider origin. Local serving needs no external-provider sign-in.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ModelProviderKind {
    /// External API; its adapter owns credentials.
    External,
    /// Runtime serving models on a published compute host.
    Local {
        /// Serving host identity.
        host_id: String,
        /// Serving runtime identity.
        runtime_id: String,
    },
}

/// Provider publication scoped to the enclosing workspace.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ModelProviderInfo {
    /// Provider identity, reusable in other workspaces.
    pub id: String,
    /// Actual provider owner.
    pub owner: String,
    /// Display label.
    pub name: String,
    /// Workspace publication revision.
    pub revision: u64,
    /// External API or local serving runtime.
    pub kind: ModelProviderKind,
    /// Last reported serving health.
    pub health: ResourceHealth,
}

/// Full model key; model IDs alone are not unique across providers.
#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ModelKey {
    /// Publishing provider.
    pub provider_id: String,
    /// Model identity within that provider.
    pub model_id: String,
}

/// Published model modality.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ModelFeature {
    /// Text generation.
    Text,
    /// Tool calling.
    Tools,
    /// Image input.
    Images,
}

/// Published serving model; presence alone says nothing about installation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ServedModelInfo {
    /// Provider-qualified identity.
    pub key: ModelKey,
    /// Display label.
    pub name: String,
    /// Workspace publication revision.
    pub revision: u64,
    /// Advertised modalities.
    pub features: Vec<ModelFeature>,
    /// Last reported inference health.
    pub health: ResourceHealth,
}

/// Permissions represented by this module. Session grants never map to these.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ResourcePermission {
    /// Establish an independently authorized compute connection.
    ConnectHost,
    /// Install/serve models on the specified host.
    InstallModel,
    /// Use models exposed by the specified provider.
    UseModels,
}

/// Permission target within the current workspace.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ResourceScope {
    /// Compute permission target.
    Host(String),
    /// Model-provider permission target.
    Provider(String),
}

/// Relevant independently revocable permissions for the authenticated audience.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ResourceGrant {
    /// Non-reusable grant identity.
    pub id: String,
    /// Current coordination revision.
    pub revision: u64,
    /// Host or provider scope; ownership never substitutes for this grant.
    pub scope: ResourceScope,
    /// Explicit permissions; no permission implies another.
    pub permissions: Vec<ResourcePermission>,
    /// Exclusive authority-clock grant expiry.
    pub expires_at_ms: Option<u64>,
    /// False after revocation, including on an already-open connection.
    pub active: bool,
}

/// Exact runtime target for a model choice, separate from its provider host.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ModelTarget {
    /// Runtime session whose model will change.
    pub session_id: String,
    /// Host executing that session.
    pub host_id: String,
    /// Runtime responsible for authorizing and confirming the choice.
    pub runtime_id: String,
    /// Current ownership epoch for Control, absent for runner sessions.
    pub control_epoch: Option<u64>,
}

/// Runtime-reported model choice and permission to attempt a new selection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ModelSelection {
    /// Exact runtime/host/session binding.
    pub target: ModelTarget,
    /// Last runtime-confirmed choice. Pending actions do not replace it.
    pub selected: Option<ModelKey>,
    /// Runtime-supplied ability for this contributor to attempt a selection.
    pub capability: ResourceCapability,
}

/// One supported runtime installation option; never an arbitrary URL or command.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ModelPackage {
    /// Package identity understood by this runtime.
    pub package_id: String,
    /// User-visible model/package label.
    pub name: String,
    /// Host on which installation and serving will occur.
    pub host_id: String,
    /// Runtime implementing this supported installation path.
    pub runtime_id: String,
}

/// Runtime facts supplied alongside coordination discovery. Empty in production
/// until real adapters are connected; development fixtures use the same contract.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ResourceRuntimeInfo {
    /// Connected adapters, independent of grants or resource registration.
    pub capabilities: ResourceCapabilities,
    /// Authorized session model choices supplied by their runtimes.
    pub selections: Vec<ModelSelection>,
    /// Supported local model installation paths.
    pub packages: Vec<ModelPackage>,
    /// Authenticated controller observation, separate from its ownership lease.
    pub controller: Option<ControllerRuntime>,
}

/// Consistent authorized discovery replacement, with separate runtime facts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ResourceSnapshot {
    /// Exact adapter/workspace/audience binding of the request.
    pub context: ResourceContext,
    /// Provider-issued visibility generation.
    pub stream_id: String,
    /// Scanned coordination position, not runtime execution order.
    pub position: u64,
    /// Current provider-aligned clock for health and lease expiry.
    pub now_ms: u64,
    /// Membership revocation disables every action regardless of resource grants.
    pub member_active: bool,
    /// Workspace-bound compute publications.
    pub hosts: Vec<ComputeHostInfo>,
    /// Workspace-bound provider publications.
    pub providers: Vec<ModelProviderInfo>,
    /// Provider-qualified published models.
    pub models: Vec<ServedModelInfo>,
    /// Only the authenticated audience's compute/provider grants.
    pub grants: Vec<ResourceGrant>,
    /// Durable workspace controller assignment and epoch watermark.
    pub controller: ControllerOwnership,
    /// Separate runtime capabilities and observations.
    pub runtime: ResourceRuntimeInfo,
}
