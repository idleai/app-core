use std::collections::BTreeMap;

use idle_history::requests::RequestTracker;

use super::{
    FreshnessStatus, ProjectionAvailability, ProjectionFilter, ProjectionFreshness, ProjectionGap,
    ProjectionKind, ProjectionLoadState, ProjectionQuery, ProjectionRow, ProjectionSelection,
    ProjectionSnapshot, ProjectionView, ViewModel,
};
use crate::{history, module::EffectError, subscriptions::Context};

/// Per-client projection interaction and correlated read state.
#[derive(Debug, Default)]
pub struct Model {
    pub(super) context: Option<Context>,
    pub(super) snapshot: Option<ProjectionSnapshot>,
    pub(super) filters: BTreeMap<ProjectionKind, ProjectionFilter>,
    pub(super) selected: Option<ProjectionSelection>,
    pub(super) load: ProjectionLoadState,
    pub(super) stale: bool,
    pub(super) suspended: bool,
    pub(super) refresh_again: bool,
    pub(super) limit: Option<u32>,
    pub(super) requests: RequestTracker<ProjectionQuery>,
    pub(super) action_error: Option<EffectError>,
    pub(super) history: Option<history::Event>,
}

impl Model {
    /// Current context; provider/audience changes retire all previous results.
    #[must_use]
    pub fn context(&self) -> Option<&Context> {
        self.context.as_ref()
    }

    pub(crate) fn take_history_event(&mut self) -> Option<history::Event> {
        self.history.take()
    }

    pub(crate) fn wait_for_connection(&mut self, context: Context) {
        if self.context() != Some(&context) {
            self.reset();
            self.context = Some(context);
        }
        self.retire_reads();
        self.suspended = true;
        self.stale = true;
        self.load = ProjectionLoadState::Suspended;
    }

    pub(super) fn row(&self, selection: &ProjectionSelection) -> Option<&ProjectionRow> {
        self.snapshot
            .as_ref()?
            .inputs
            .iter()
            .find(|input| input.kind == selection.kind)?
            .rows
            .iter()
            .find(|row| row.key == selection.key)
    }

    pub(super) fn reset(&mut self) {
        self.retire_reads();
        self.context = None;
        self.snapshot = None;
        self.filters.clear();
        self.selected = None;
        self.load = ProjectionLoadState::Idle;
        self.stale = false;
        self.suspended = false;
        self.limit = None;
        self.action_error = None;
        self.history = None;
    }

    pub(super) fn retire_reads(&mut self) {
        self.requests.clear();
        self.refresh_again = false;
    }

    fn destination(&self, kind: ProjectionKind) -> ProjectionView {
        let filter = self.filters.get(&kind).cloned().unwrap_or_default();
        let input = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.inputs.iter().find(|input| input.kind == kind));
        let Some(input) = input else {
            return ProjectionView {
                kind,
                filter,
                rows: Vec::new(),
                total: None,
                loaded_count: 0,
                visible_count: 0,
                availability: ProjectionAvailability::Unavailable,
                freshness: ProjectionFreshness {
                    status: FreshnessStatus::Unknown,
                    generated_at_ms: None,
                    checkpoint: None,
                },
                gaps: vec![ProjectionGap {
                    reference: None,
                    message: "Projection input has not been loaded".into(),
                }],
            };
        };
        let rows: Vec<_> = input
            .rows
            .iter()
            .filter(|row| filter.matches(row))
            .cloned()
            .collect();
        let mut freshness = input.freshness.clone();
        if self.stale {
            freshness.status = FreshnessStatus::Stale;
        }
        ProjectionView {
            kind,
            filter,
            visible_count: u64::try_from(rows.len()).unwrap_or(u64::MAX),
            loaded_count: u64::try_from(input.rows.len()).unwrap_or(u64::MAX),
            rows,
            total: input.total,
            availability: input.availability,
            freshness,
            gaps: input.gaps.clone(),
        }
    }

    pub(super) fn view(&self) -> ViewModel {
        ViewModel {
            context: self.context.clone(),
            load: self.load.clone(),
            needs_refresh: self.stale,
            selected: self.selected.clone(),
            action_error: self.action_error.clone(),
            activity: self.destination(ProjectionKind::Activity),
            tasks: self.destination(ProjectionKind::Task),
            errors: self.destination(ProjectionKind::Error),
            triage: self.destination(ProjectionKind::Triage),
            need_input: self.destination(ProjectionKind::NeedInput),
        }
    }
}
