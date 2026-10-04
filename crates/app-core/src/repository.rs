//! Repository reads and recorded-session selection, separate from live runtime state.

mod reducer;
#[cfg(test)]
mod tests;
mod types;

use idle_history::requests::RequestTracker;

pub use idle_protocol::v1::repository::{
    GitAuthor, GitCheckout, GithubPerson, GithubRepository, ReadReport, ReadState, RecordedSession,
    RepositoryScope, RepositorySnapshot, SourceRecord, WorktreeStatus,
};
pub use reducer::{Repositories, RepositoryEvent as Event};
pub use types::{
    RepositoryAction, RepositoryContext, RepositoryLoadState, RepositoryOutput, RepositoryQuery,
    RepositoryResponse, RepositoryResult, RepositoryViewModel as ViewModel,
};

/// Per-client repository state and correlated, non-reusable read identities.
#[derive(Debug, Default)]
pub struct Model {
    context: Option<RepositoryContext>,
    snapshot: Option<RepositorySnapshot>,
    selected: Option<String>,
    load: RepositoryLoadState,
    stale: bool,
    refresh_again: Option<RepositoryAction>,
    selection_initialized: bool,
    requests: RequestTracker<RepositoryQuery>,
    action_error: Option<crate::module::EffectError>,
    history: Vec<crate::history::Event>,
}

impl Model {
    /// Current provider, audience and explicit repository binding.
    #[must_use]
    pub fn context(&self) -> Option<&RepositoryContext> {
        self.context.as_ref()
    }

    pub(crate) fn selected_session(&self) -> Option<&str> {
        self.selected.as_deref()
    }
    pub(crate) fn take_history_events(&mut self) -> Vec<crate::history::Event> {
        std::mem::take(&mut self.history)
    }

    pub(crate) fn wait_for_connection(&mut self, context: RepositoryContext) {
        if self.context.as_ref() != Some(&context) {
            self.reset();
            self.context = Some(context);
        }
        self.requests.clear();
        self.refresh_again = None;
        self.stale = true;
        self.load = RepositoryLoadState::Suspended;
    }

    fn reset(&mut self) {
        self.requests.clear();
        self.context = None;
        self.snapshot = None;
        self.selected = None;
        self.load = RepositoryLoadState::Idle;
        self.stale = false;
        self.refresh_again = None;
        self.action_error = None;
        self.selection_initialized = false;
        self.history.clear();
    }
}
