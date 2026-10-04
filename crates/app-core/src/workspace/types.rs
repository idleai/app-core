//! Portable host operations and navigation views, independent of coordination SDKs.

use crux_core::capability::Operation;
use serde::{Deserialize, Serialize};

/// Adapter supplying workspace metadata; independent of model-provider sign-in.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum WorkspaceMode {
    /// One Git repository, local metadata and peer coordination without Offstage.
    Standalone,
    /// Managed coordination for zero or more repositories.
    Managed,
}

/// Repository identity and display metadata, reusable across workspace attachments.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection helpers do not impose safety invariants on these fields"
)]
pub struct RepositoryInfo {
    /// Stable repository identity, never a checkout path.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Credential-free canonical Git locator, when published.
    pub remote: Option<String>,
}

/// Authorized workspace metadata and its single logical chain binding.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection helpers do not impose safety invariants on these fields"
)]
pub struct WorkspaceInfo {
    /// Stable workspace identity, preserved when adopting managed coordination.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Opaque engine-issued logical reference, not a storage URL or workspace ID.
    pub chain: String,
    /// Authority-assigned workspace metadata revision.
    pub revision: u64,
    /// Coordination adapter to use for this workspace.
    pub mode: WorkspaceMode,
    /// Exactly one repository in standalone mode; zero or more in managed mode.
    pub repositories: Vec<RepositoryInfo>,
}

pub use idle_history::binding::RepositoryChainBinding;

/// Metadata role only; no role grants compute, model or session access.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum MemberRole {
    /// Workspace owner.
    Owner,
    /// Membership/configuration administrator.
    Admin,
    /// Workspace participant.
    Member,
    /// Read-only observer.
    Viewer,
}

/// Membership is independent of whether a contributor is online.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum MemberStatus {
    /// Current membership, still subject to resource grants.
    Active,
    /// Membership has been revoked; peer activity is suppressed.
    Revoked,
}

/// One contributor's membership in the enclosing workspace.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection helpers do not impose safety invariants on these fields"
)]
pub struct MemberInfo {
    /// Human or service identity, distinct from host and session owners.
    pub contributor_id: String,
    /// Presentation label; falls back to the ID when directory metadata is absent.
    pub display_name: String,
    /// Authority-assigned membership revision.
    pub revision: u64,
    /// Workspace metadata role.
    pub role: MemberRole,
    /// Current membership state.
    pub status: MemberStatus,
}

/// Consistent authorized metadata for one workspace.
///
/// Resource IDs are bindings in this scope, not global ownership or access grants.
/// Resource operation/availability state belongs to the resources domain.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection helpers do not impose safety invariants on these fields"
)]
pub struct WorkspaceSnapshot {
    /// Exact workspace and repository bindings at this snapshot.
    pub workspace: WorkspaceInfo,
    /// Visible active and revoked memberships.
    pub members: Vec<MemberInfo>,
    /// Visible compute host bindings; IDs may also occur in other workspaces.
    pub host_ids: Vec<String>,
    /// Visible model provider bindings; IDs may also occur in other workspaces.
    pub provider_ids: Vec<String>,
}

/// Member connection status, with unknown distinct from explicitly offline.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum PresenceStatus {
    /// No unexpired observation is available.
    #[default]
    Unknown,
    /// Contributor is actively connected.
    Online,
    /// Contributor is connected but away.
    Away,
    /// The adapter explicitly observed a disconnect.
    Offline,
}

/// A contributor may have several simultaneous connections on different hosts.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection helpers do not impose safety invariants on these fields"
)]
pub struct PresenceEntry {
    /// Connection identity unique inside this workspace's peer activity snapshot.
    pub connection_id: String,
    /// Actual contributor, never inferred from a host label.
    pub contributor_id: String,
    /// Reported connection state.
    pub status: PresenceStatus,
    /// Repository being worked on, if reported.
    pub repository_id: Option<String>,
    /// Branch name within the reported repository.
    pub branch: Option<String>,
    /// Repository-relative file path, if reported.
    pub file: Option<String>,
    /// Host in this workspace's authorized discovery bindings, if reported.
    pub host_id: Option<String>,
    /// Supplied work description, without inferred activity.
    pub summary: Option<String>,
    /// Producer observation time in Unix milliseconds, never a recovery cursor.
    pub observed_at_ms: u64,
    /// Exclusive freshness deadline in the same clock domain.
    pub valid_until_ms: u64,
}

/// Full peer activity replacement from the selected workspace's adapter.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection helpers do not impose safety invariants on these fields"
)]
pub struct PresenceSnapshot {
    /// Workspace scope; must match the pending operation.
    pub workspace_id: String,
    /// Adapter's current Unix time used to evaluate freshness.
    pub as_of_ms: u64,
    /// Complete currently visible connections, including explicit offline reports.
    pub entries: Vec<PresenceEntry>,
}

/// Read-only operations executed by authenticated host coordination adapters.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum WorkspaceOperation {
    /// Discover all currently available authorized standalone/managed workspaces.
    /// The host supplies its configured connections; no sign-in is implied.
    List,
    /// Fetch a consistent workspace/member/resource-binding snapshot.
    Snapshot {
        /// Exact workspace scope.
        workspace_id: String,
        /// Coordination route selected from the directory.
        mode: WorkspaceMode,
    },
    /// Fetch current peer activity through the same authorized provider.
    Presence {
        /// Exact workspace scope.
        workspace_id: String,
        /// Coordination route selected from accepted workspace metadata.
        mode: WorkspaceMode,
    },
}

/// Host results are validated against their operation and workspace scope.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum WorkspaceResult {
    /// Authorized directory replacement, including a known-empty directory.
    Directory(Vec<WorkspaceInfo>),
    /// Selected workspace metadata.
    Snapshot(WorkspaceSnapshot),
    /// Selected workspace peer activity.
    Presence(PresenceSnapshot),
}

/// Failure classification for safe loading and retry presentation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum WorkspaceErrorKind {
    /// Provider is temporarily unavailable; cached metadata may remain visible.
    Unavailable,
    /// The host needs an authenticated coordination connection.
    Unauthenticated,
    /// Workspace access was denied or revoked.
    Forbidden,
    /// The workspace is absent or no longer visible.
    NotFound,
    /// Provider does not implement this capability.
    Unsupported,
    /// Host data violates scope, identity or binding invariants.
    InvalidData,
    /// The requested navigation target is absent from current metadata.
    InvalidSelection,
}

/// Presentable coordination error, without credentials or private diagnostics.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection helpers do not impose safety invariants on these fields"
)]
pub struct WorkspaceError {
    /// How the view should classify the failure.
    pub kind: WorkspaceErrorKind,
    /// Safe explanation supplied by the host or reducer.
    pub message: String,
}

/// Typed Rust host output.
pub type WorkspaceOutput = Result<WorkspaceResult, WorkspaceError>;

/// Named result for generated Swift/Kotlin codecs; matches [`WorkspaceOutput`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum WorkspaceResponse {
    /// Successful host response.
    Ok(WorkspaceResult),
    /// Explicit provider or validation failure.
    Err(WorkspaceError),
}

impl Operation for WorkspaceOperation {
    type Output = WorkspaceOutput;
}

/// Loading status independent of last successfully loaded metadata.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum WorkspaceRequestState {
    /// No request has been made.
    #[default]
    Idle,
    /// Awaiting an adapter result; retained data is stale.
    Loading,
    /// The last request succeeded, possibly with empty data.
    Ready,
    /// The last request failed; retry is an explicit client action.
    Failed(WorkspaceError),
}

/// Shared navigation destinations; renderers choose their visual arrangement.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum NavigationSection {
    /// Workspace and repository overview.
    #[default]
    Workspace,
    /// Members and their reported activity.
    Members,
    /// Owned and invited sessions, including Control.
    Sessions,
    /// Workspace projections.
    Projections,
    /// Published compute hosts.
    ComputeHosts,
    /// Published model providers.
    ModelProviders,
    /// History of the selected logical chain.
    Activity,
    /// General workspace settings.
    Settings,
    /// Agent rules, kept separate from settings.
    AgentRules,
}

/// Member identity combined with fresh, workspace-scoped observations.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection helpers do not impose safety invariants on these fields"
)]
pub struct MemberView {
    /// Membership facts, including role and revocation.
    pub member: MemberInfo,
    /// Online takes precedence over away, then explicitly offline, then unknown.
    pub presence: PresenceStatus,
    /// Unexpired online/away connections only; offline/unknown locations are hidden.
    pub connections: Vec<PresenceEntry>,
}

/// Workspace state consumed by every client surface.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection helpers do not impose safety invariants on these fields"
)]
pub struct WorkspaceViewModel {
    /// Authorized workspace choices; freshness is given by `directory_state`.
    pub workspaces: Vec<WorkspaceInfo>,
    /// Workspace discovery progress/error.
    pub directory_state: WorkspaceRequestState,
    /// Selected workspace identity, independent of the selected repository.
    pub selected_workspace: Option<String>,
    /// Selected repository; `None` means workspace-wide in managed mode.
    pub selected_repository: Option<String>,
    /// Only the opaque chain reference is forwarded to the engine.
    pub chain: Option<String>,
    /// Resolved binding for the selected repository, when one is selected.
    pub repository_binding: Option<RepositoryChainBinding>,
    /// Explicit bindings for all repositories in the visible workspace directory.
    pub repository_bindings: Vec<RepositoryChainBinding>,
    /// Shared navigation selection.
    pub section: NavigationSection,
    /// Last successful selected workspace snapshot; load state shows staleness.
    pub snapshot: Option<WorkspaceSnapshot>,
    /// Selected workspace/member loading progress/error.
    pub snapshot_state: WorkspaceRequestState,
    /// Members with fresh peer activity, without conflating humans and hosts.
    pub members: Vec<MemberView>,
    /// Independent peer activity loading progress/error.
    pub presence_state: WorkspaceRequestState,
    /// Invalid user selection without discarding a valid current context.
    pub selection_error: Option<WorkspaceError>,
}
