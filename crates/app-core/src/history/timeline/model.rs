//! Semantic state, independent of viewport pixels and native graph planning.

use std::collections::{BTreeMap, BTreeSet};

use idle_history::{
    requests::RequestTracker,
    timeline::{Action, Cursor, Match, Progress, Target, View, Window},
};
use serde::{Deserialize, Serialize};

use crate::history::{Filter, QueryAction, RequestState};

/// Independent history composition sharing exact selection and native actions.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Deserialize,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Serialize,
    facet::Facet,
)]
#[repr(u8)]
#[facet(rename = "TimelineSurface")]
pub enum Surface {
    /// Full Activity editor.
    #[default]
    Editor,
    /// Compact sidebar.
    Mini,
}

/// Exact selected occurrence and its native destination.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants to these wire fields"
)]
#[facet(rename = "TimelineSelection")]
pub struct Selection {
    /// Stable activity occurrence.
    pub occurrence: String,
    /// Exact stored record or repository-qualified Git commit.
    pub address: Target,
}

/// In-place Find state and globally indexed match position.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants to these wire fields"
)]
#[facet(rename = "TimelineSearch")]
pub struct Search {
    /// Submitted literal text.
    pub text: String,
    /// Read status, preserving readable timeline rows on failure.
    pub state: RequestState,
    /// Current bounded match page.
    pub matches: Vec<Match>,
    /// Exact global match count.
    pub total: u64,
    /// First match position in the loaded page.
    pub offset: u64,
    /// Current global match position.
    pub current: Option<u64>,
    /// Host cursor used for indexed navigation between match pages.
    pub next: Option<Cursor>,
    /// Records whose complete searchable text is unavailable.
    pub unavailable: u64,
}

/// One composition's cached rows and semantic interaction state.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants to these wire fields"
)]
#[facet(rename = "TimelineSurfaceView")]
pub struct SurfaceView {
    /// Last readable window; failures never replace this with an empty table.
    pub window: Option<Window>,
    /// Window read status.
    pub state: RequestState,
    /// Current source filters.
    pub filter: Filter,
    /// Independent Find state.
    pub search: Search,
    /// Cancellable native index progress.
    pub progress: Option<Progress>,
    /// Exact semantic destination to reveal after navigation or Find.
    pub focus: Option<String>,
    /// Changes for every explicit viewport destination, including a repeated seek.
    pub focus_revision: u64,
    /// New activities arrived while reading an older position.
    pub new_activity: bool,
    /// Serialized bytes currently retained for this composition's timeline.
    pub cached_bytes: u64,
}

/// Shared timeline selection with independent editor and mini windows.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[facet(rename = "TimelineViewModel")]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants to these wire fields"
)]
pub struct ViewModel {
    /// Full editor state.
    pub editor: SurfaceView,
    /// Compact sidebar state.
    pub mini: SurfaceView,
    /// Exact selection shared by the two compositions.
    pub selected: Option<Selection>,
    /// Latest native activation status.
    pub open: RequestState,
    /// Explanation for a native JSON fallback or unavailable action.
    pub notice: Option<String>,
}

#[derive(Debug, Default)]
pub(super) struct State {
    pub view: SurfaceView,
    pub active: bool,
    pub manual: BTreeMap<String, bool>,
    pub temporary: BTreeSet<String>,
    pub reading: BTreeSet<String>,
    pub visible: BTreeSet<String>,
    pub anchor: Option<String>,
    pub at_newest: bool,
    pub intent: Option<Action>,
    pub resume: Option<Action>,
    pub pending_move: Option<i32>,
}

impl State {
    pub(super) fn query_view(&self) -> View {
        let mut choices = self.manual.clone();
        for id in &self.reading {
            if !self.manual.contains_key(id) {
                let _previous = choices.insert(id.clone(), true);
            }
        }
        for id in &self.temporary {
            let _previous = choices.insert(id.clone(), true);
        }
        View {
            filter: self.view.filter.clone(),
            disclosures: choices
                .into_iter()
                .map(|(group, expanded)| idle_history::timeline::Disclosure { group, expanded })
                .collect(),
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct Pending {
    pub surface: Surface,
    pub query: QueryAction,
}

#[derive(Debug, Default)]
pub(in crate::history) struct Model {
    pub(super) chain: Option<String>,
    pub(super) surfaces: BTreeMap<Surface, State>,
    pub(super) selected: Option<Selection>,
    pub(super) open: RequestState,
    pub(super) notice: Option<String>,
    pub(super) pending: RequestTracker<Pending>,
}

impl Model {
    pub(in crate::history) fn bind(&mut self, chain: Option<String>) {
        // A subscription may finish binding after a mounted surface starts its
        // first read. Retire that read while keeping the surface eligible for
        // the subscription's subsequent refresh.
        let surfaces = if chain.is_some() && self.chain == chain {
            self.surfaces
                .iter()
                .filter(|(_, state)| state.active)
                .map(|(surface, _)| {
                    (
                        *surface,
                        State {
                            active: true,
                            at_newest: true,
                            ..State::default()
                        },
                    )
                })
                .collect()
        } else {
            BTreeMap::new()
        };
        self.pending.clear();
        *self = Self {
            chain,
            surfaces,
            pending: std::mem::take(&mut self.pending),
            ..Self::default()
        };
    }

    pub(in crate::history) fn view(&self) -> ViewModel {
        let get = |surface| {
            self.surfaces
                .get(&surface)
                .map(|state| state.view.clone())
                .unwrap_or_default()
        };
        ViewModel {
            editor: get(Surface::Editor),
            mini: get(Surface::Mini),
            selected: self.selected.clone(),
            open: self.open.clone(),
            notice: self.notice.clone(),
        }
    }
}
