//! Typed view state; client filters never change the supplied scope totals.

use serde::{Deserialize, Serialize};

use super::{
    ProjectionAvailability, ProjectionFreshness, ProjectionGap, ProjectionKind, ProjectionRow,
};
use crate::{module::EffectError, subscriptions::Context};

/// Local conjunctive filters over supplied display fields.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ProjectionFilter {
    /// Literal case-sensitive substring in the supplied title or summary.
    pub text: String,
    /// Exact opaque provider status, when selected.
    pub status: Option<String>,
    /// Every requested label must be present on a row.
    pub labels: Vec<String>,
}

impl ProjectionFilter {
    pub(super) fn matches(&self, row: &ProjectionRow) -> bool {
        (self.text.is_empty()
            || row.title.contains(&self.text)
            || row
                .summary
                .as_ref()
                .is_some_and(|text| text.contains(&self.text)))
            && self
                .status
                .as_ref()
                .is_none_or(|status| row.status.as_ref() == Some(status))
            && self.labels.iter().all(|label| row.labels.contains(label))
    }
}

/// Stable selection within one destination; source IDs remain on its row.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ProjectionSelection {
    /// Selected destination.
    pub kind: ProjectionKind,
    /// Provider-assigned row key, independent of physical observation updates.
    pub key: String,
}

/// Replacement-read lifecycle, independent of each result's completeness.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ProjectionLoadState {
    /// No context has been loaded.
    #[default]
    Idle,
    /// An atomic replacement is pending; old rows may remain visible as stale.
    Loading,
    /// Host supplied and validated all five inputs.
    Ready,
    /// Transport continuity was lost; old rows remain stale until reconnect.
    Suspended,
    /// Read failed; a refresh may retry.
    Failed(EffectError),
}

/// One filtered list/board, suitable for compact and wide presentations.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ProjectionView {
    /// Presentation destination.
    pub kind: ProjectionKind,
    /// Active client filter.
    pub filter: ProjectionFilter,
    /// Rows in supplied order after filtering.
    pub rows: Vec<ProjectionRow>,
    /// Supplied scope total, not changed by local filtering.
    pub total: Option<u64>,
    /// Number of rows loaded before filtering, possibly below the scope total.
    pub loaded_count: u64,
    /// Number of rows visible after local filtering.
    pub visible_count: u64,
    /// Completeness supplied by the provider.
    pub availability: ProjectionAvailability,
    /// Provider details, marked stale after a local invalidation or failed refresh.
    pub freshness: ProjectionFreshness,
    /// Supplied limitations and exact addresses where available.
    pub gaps: Vec<ProjectionGap>,
}

/// Five independent destinations sharing one replacement and context lifetime.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ProjectionViewModel {
    /// Current authorized provider, contributor, workspace and chain.
    pub context: Option<Context>,
    /// Host read state; unavailable destinations are reported individually.
    pub load: ProjectionLoadState,
    /// Whether retained results require a replacement read.
    pub needs_refresh: bool,
    /// Stable selection, retained through filters and same-context refreshes.
    pub selected: Option<ProjectionSelection>,
    /// Recorded activity list.
    pub activity: ProjectionView,
    /// Supplied task list or board.
    pub tasks: ProjectionView,
    /// Supplied error list.
    pub errors: ProjectionView,
    /// Supplied triage list.
    pub triage: ProjectionView,
    /// Supplied requests for human input.
    pub need_input: ProjectionView,
    /// Invalid client selection or drill-down, separate from read failures.
    pub action_error: Option<EffectError>,
}

impl Default for ProjectionViewModel {
    fn default() -> Self {
        super::Model::default().view()
    }
}
