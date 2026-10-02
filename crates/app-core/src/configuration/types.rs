//! Portable documents and independent editor views.

use serde::{Deserialize, Serialize};

use super::{ConfigurationError, ConfigurationSave};
use crate::workspace::WorkspaceMode;

/// JSON object document format understood by the editor, separate from revisions.
pub const DOCUMENT_VERSION: u32 = 1;

/// Exact coordination route and authenticated workspace audience.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ConfigurationContext {
    /// Host-configured adapter name, without credentials.
    pub provider: String,
    /// Workspace owning the configuration.
    pub workspace_id: String,
    /// Authenticated contributor, independent of provider credentials.
    pub contributor_id: String,
    /// Workspace's logical chain binding.
    pub chain: String,
    /// Standalone or managed coordination.
    pub mode: WorkspaceMode,
}

/// Separate navigation and revision scopes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ConfigurationDocument {
    /// General workspace settings.
    Settings,
    /// Agent rules interpreted and enforced by Evo.
    AgentRules,
}

/// Opaque domain configuration; the editor does not interpret policy fields.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ConfigurationValue {
    /// Document format version, independent of the provider's revision.
    pub schema_version: u32,
    /// Complete JSON object text, retaining fields unknown to this client.
    pub json: String,
}

impl Default for ConfigurationValue {
    fn default() -> Self {
        Self {
            schema_version: DOCUMENT_VERSION,
            json: "{}".into(),
        }
    }
}

/// Provider-confirmed value; drafts never manufacture a revision.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ConfigurationRecord {
    /// Positive, increasing revision within this workspace/document scope.
    pub revision: u64,
    /// Complete persisted value.
    pub value: ConfigurationValue,
}

/// Authorized read or committed replacement from either provider.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ConfigurationSnapshot {
    /// Exact route, scope and audience of the operation.
    pub context: ConfigurationContext,
    /// Requested settings or rules scope.
    pub document: ConfigurationDocument,
    /// None only if this document has never existed in this scope.
    pub record: Option<ConfigurationRecord>,
    /// Provider-supplied ability to attempt a write; every write is reauthorized.
    pub can_edit: bool,
}

/// Loading state independent of pending edits and save feedback.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ConfigurationLoadState {
    /// No selected context.
    #[default]
    Idle,
    /// An initial or recovery read is in flight; saving waits for its result.
    Loading,
    /// Current read completed, including an explicitly absent document.
    Ready,
    /// Delivery continuity was lost; reload before saving.
    Suspended,
    /// Read failed; any previous draft is retained.
    Failed(ConfigurationError),
    /// A background read is in flight; the confirmed value remains usable.
    Refreshing,
}

/// Save feedback never treats dispatch or receipt as a successful commit.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ConfigurationSaveState {
    /// No save feedback yet, or a draft was discarded.
    #[default]
    Idle,
    /// Original immutable save is in flight.
    Saving,
    /// Provider confirmed the committed revision; newer edits may still be pending.
    Saved(u64),
    /// Provider confirmed rejection, or the client intent was invalid.
    Failed(ConfigurationError),
    /// Outcome is unknown; only the unchanged original request may be retried.
    Uncertain(ConfigurationError),
}

/// Client intents currently available for a document editor.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ConfigurationEditorAction {
    /// Edit the current draft, including while saving or temporarily disconnected.
    Edit,
    /// Submit the valid changed draft with a new request identity.
    Save,
    /// Recover an uncertain save using its unchanged identity and payload.
    RetrySave,
    /// Replace the draft with the latest confirmed document.
    Discard,
    /// Retain a reviewed draft against the latest confirmed revision.
    Rebase,
}

/// One document's current value, draft and independent asynchronous feedback.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ConfigurationEditorView {
    /// Latest provider-confirmed value; None may mean absent or not loaded yet.
    pub current: Option<ConfigurationRecord>,
    /// Saved revision from which this draft was edited; None means create-if-absent.
    pub base_revision: Option<u64>,
    /// Full editable content, including invalid intermediate text.
    pub draft: ConfigurationValue,
    /// Whether the draft differs from its original saved value.
    pub dirty: bool,
    /// Newer provider content differs from the draft's base; explicit review needed.
    pub conflict: bool,
    /// Syntactic document validation only, never a policy decision.
    pub validation_error: Option<ConfigurationError>,
    /// Read state, separate from save state.
    pub load: ConfigurationLoadState,
    /// Current or most recent save outcome.
    pub save: ConfigurationSaveState,
    /// Original in-flight or uncertain payload; newer edits cannot mutate it.
    pub pending: Option<ConfigurationSave>,
    /// Available client actions; providers still authorize every effect.
    pub actions: Vec<ConfigurationEditorAction>,
}

/// Shared settings and rules state consumed by every client surface.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ConfigurationViewModel {
    /// Active provider/workspace/audience binding.
    pub context: Option<ConfigurationContext>,
    /// General settings editor.
    pub settings: ConfigurationEditorView,
    /// Agent rules editor with its own revision and draft.
    pub agent_rules: ConfigurationEditorView,
    /// Invalid context or client action, independent of provider feedback.
    pub action_error: Option<ConfigurationError>,
}
