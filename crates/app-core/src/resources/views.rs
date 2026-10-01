//! Renderer-independent availability, actions and pending/error feedback.

use serde::{Deserialize, Serialize};

use super::{
    ComputeHostInfo, ControllerView, ModelPackage, ModelProviderInfo, ModelSelection, ModelTarget,
    ResourceAvailability, ResourceCapabilities, ResourceContext, ResourceError, ResourceMutation,
    ResourcePermission, ResourceProgress, ResourceRequest, ServedModelInfo,
};

/// Shared directory load state. A failed refresh retains rows with unknown health.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ResourceLoadState {
    /// No resource context selected.
    #[default]
    Idle,
    /// A replacement is pending; mutation actions are disabled.
    Loading,
    /// Authorized resource discovery is current.
    Ready,
    /// Delivery continuity is lost; pending mutations require status recovery.
    Suspended,
    /// The host reported an error or returned invalid data.
    Failed(ResourceError),
}

/// Host row with currently permitted and supported actions.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ComputeHostView {
    /// Publication details, including its actual owner.
    pub host: ComputeHostInfo,
    /// Health after freshness and connection checks.
    pub availability: ResourceAvailability,
    /// Supported actions after independent compute grants and pending work.
    pub actions: Vec<ResourcePermission>,
}

/// Provider row; credentials and managed sign-in are not part of this state.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ModelProviderView {
    /// Workspace publication.
    pub provider: ModelProviderInfo,
    /// Effective provider and, for local serving, compute health.
    pub availability: ResourceAvailability,
    /// Independent provider actions supported by the connected runtime.
    pub actions: Vec<ResourcePermission>,
}

/// Provider-qualified model and exact runtime targets allowed to attempt selection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ServedModelView {
    /// Published model details.
    pub model: ServedModelInfo,
    /// Effective model, provider and local-host health.
    pub availability: ResourceAvailability,
    /// Permitted runtime targets; an empty list disables selection.
    pub selectable_for: Vec<ModelTarget>,
}

/// Supported package and whether its host currently permits installation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ModelPackageView {
    /// Runtime-supplied installation option.
    pub package_info: ModelPackage,
    /// True only while compute grants, health and runtime support permit an attempt.
    pub can_install: bool,
}

/// Safe next step for an existing action, always retaining its original identity.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ResourceRecoveryAction {
    /// Look up a running or uncertain result; never resubmit it.
    CheckStatus,
    /// Retry only when the adapter explicitly permits the unchanged request.
    Retry,
}

/// Mutation history for the current context. A receipt stays pending until a
/// runtime reports a terminal stage; effect failures preserve any last known fact.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ResourceMutationView {
    /// Original persisted retry identity and deadline.
    pub request: ResourceRequest,
    /// Exact original intent.
    pub mutation: ResourceMutation,
    /// Last authenticated action fact, absent before a confirmed response.
    pub progress: Option<ResourceProgress>,
    /// True until runtime success or terminal runtime failure is confirmed.
    pub pending: bool,
    /// Whether a mutation/status effect is currently awaiting its response.
    pub in_flight: bool,
    /// Last transport or reconciliation error, separate from runtime failure.
    pub error: Option<ResourceError>,
    /// Safe recovery controls currently available.
    pub recovery: Vec<ResourceRecoveryAction>,
}

/// Shared compute/model/controller screen state for every client surface.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ResourceViewModel {
    /// Current workspace/provider/audience, if connected.
    pub context: Option<ResourceContext>,
    /// Discovery load/error state.
    pub load: ResourceLoadState,
    /// Runtime capabilities; unavailable until connected and discovery is current.
    pub capabilities: ResourceCapabilities,
    /// Local host navigation, not a runtime connection or compute grant.
    pub selected_host: Option<String>,
    /// Local provider navigation, not runtime model adoption.
    pub selected_provider: Option<String>,
    /// Compute publications with effective availability and actions.
    pub hosts: Vec<ComputeHostView>,
    /// Provider publications with effective availability and actions.
    pub providers: Vec<ModelProviderView>,
    /// Published models and exact allowed selection targets.
    pub models: Vec<ServedModelView>,
    /// Last runtime-confirmed choices, preserved while another choice is pending.
    pub selections: Vec<ModelSelection>,
    /// Runtime-supplied installation catalog and allowed installation actions.
    pub packages: Vec<ModelPackageView>,
    /// Controller assignment, runtime phase and health.
    pub controller: ControllerView,
    /// Current-context mutations with independent progress/errors/recovery.
    pub mutations: Vec<ResourceMutationView>,
    /// Invalid local action feedback.
    pub action_error: Option<ResourceError>,
}
