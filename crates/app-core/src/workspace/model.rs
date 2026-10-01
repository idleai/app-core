//! Per-client workspace state and derived navigation views.

use std::collections::BTreeMap;

use super::{
    MemberInfo, MemberStatus, MemberView, PresenceEntry, PresenceStatus, RepositoryChainBinding,
    WorkspaceInfo, WorkspaceOperation, WorkspaceViewModel,
};

/// Client-local navigation and pending coordination work, with no platform I/O.
#[derive(Debug, Default)]
pub struct Model {
    pub(super) view: WorkspaceViewModel,
    pub(super) pending: BTreeMap<u64, WorkspaceOperation>,
    pub(super) next_request: u64,
    // Preserve immutable bindings/revisions even across removal and reconnect.
    pub(super) known: BTreeMap<String, WorkspaceInfo>,
    pub(super) presence: Vec<PresenceEntry>,
    pub(super) now_ms: u64,
    pub(super) connected: bool,
}

impl Model {
    /// Resolve the selected workspace to its logical engine reference only.
    #[must_use]
    pub fn chain(&self) -> Option<&str> {
        self.selected().map(|info| info.chain.as_str())
    }

    pub(crate) const fn owns_history(&self) -> bool {
        self.connected
    }

    pub(super) fn selected(&self) -> Option<&WorkspaceInfo> {
        self.view
            .workspaces
            .iter()
            .find(|info| Some(&info.id) == self.view.selected_workspace.as_ref())
    }

    pub(super) fn render(&self) -> WorkspaceViewModel {
        let mut view = self.view.clone();
        view.chain = self.chain().map(str::to_owned);
        view.repository_bindings = view
            .workspaces
            .iter()
            .flat_map(|info| {
                info.repositories.iter().map(|repo| RepositoryChainBinding {
                    workspace_id: info.id.clone(),
                    repository_id: repo.id.clone(),
                    chain: info.chain.clone(),
                })
            })
            .collect();
        view.repository_binding = view
            .repository_bindings
            .iter()
            .find(|binding| {
                Some(&binding.workspace_id) == view.selected_workspace.as_ref()
                    && Some(&binding.repository_id) == view.selected_repository.as_ref()
            })
            .cloned();
        view.members = view.snapshot.as_ref().map_or_else(Vec::new, |snapshot| {
            snapshot
                .members
                .iter()
                .map(|member| self.member_view(member))
                .collect()
        });
        view
    }

    fn member_view(&self, member: &MemberInfo) -> MemberView {
        let fresh: Vec<_> = self
            .presence
            .iter()
            .filter(|entry| {
                member.status == MemberStatus::Active
                    && entry.contributor_id == member.contributor_id
                    && entry.observed_at_ms <= self.now_ms
                    && self.now_ms < entry.valid_until_ms
            })
            .collect();
        let presence = [
            PresenceStatus::Online,
            PresenceStatus::Away,
            PresenceStatus::Offline,
        ]
        .into_iter()
        .find(|status| fresh.iter().any(|entry| entry.status == *status))
        .unwrap_or_default();
        let connections = fresh
            .into_iter()
            .filter(|entry| matches!(entry.status, PresenceStatus::Online | PresenceStatus::Away))
            .cloned()
            .collect();
        MemberView {
            member: member.clone(),
            presence,
            connections,
        }
    }
}
