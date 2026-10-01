//! Client-local state and effective participation views.

use std::collections::BTreeMap;

use idle_history::requests::RequestTracker;

use crate::workspace::{MemberInfo, MemberStatus};

use super::{
    SessionCapabilities, SessionCapability, SessionContext, SessionError, SessionGrant,
    SessionGrantStatus, SessionInfo, SessionItemBinding, SessionLoadState, SessionMutationView,
    SessionOperation, SessionPermission, SessionPromptView, SessionRelationship, SessionSnapshot,
    SessionView, SessionViewModel,
};

/// One client's session selection, pending requests and runtime-confirmed facts.
#[derive(Debug, Default)]
pub struct Model {
    pub(super) state: State,
    pub(super) requests: RequestTracker<SessionOperation>,
    // Retain only immutable bindings across provider/audience changes, not prompts.
    pub(super) bindings: BTreeMap<(String, String), SessionItemBinding>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct State {
    pub context: Option<SessionContext>,
    pub snapshot: Option<SessionSnapshot>,
    pub selected: Option<String>,
    pub load: SessionLoadState,
    pub updates: SessionLoadState,
    pub now_ms: u64,
    pub prompts: Vec<SessionPromptView>,
    pub mutations: Vec<SessionMutationView>,
    pub action_error: Option<SessionError>,
    // Revision floors survive same-context snapshots that omit an entity.
    pub known_sessions: BTreeMap<String, SessionInfo>,
    pub known_grants: BTreeMap<String, SessionGrant>,
    pub known_members: BTreeMap<String, MemberInfo>,
}

impl Model {
    /// Current authenticated context, if connected.
    #[must_use]
    pub fn context(&self) -> Option<&SessionContext> {
        self.state.context.as_ref()
    }
}

impl State {
    pub(super) fn member_active(&self, id: &str) -> bool {
        self.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot
                .members
                .iter()
                .any(|member| member.contributor_id == id && member.status == MemberStatus::Active)
        })
    }

    pub(super) fn ready(&self) -> bool {
        self.load == SessionLoadState::Ready && !matches!(self.updates, SessionLoadState::Failed(_))
    }

    pub(super) fn permissions(&self, session: &SessionInfo) -> Vec<SessionPermission> {
        let Some(context) = &self.context else {
            return Vec::new();
        };
        if !self.member_active(&context.contributor_id) {
            return Vec::new();
        }
        let mut permissions = Vec::new();
        if session.owner == context.contributor_id {
            // Ownership permits observing/sharing metadata, not runtime or compute use.
            permissions.extend([SessionPermission::Observe, SessionPermission::Invite]);
        }
        if let Some(snapshot) = &self.snapshot {
            for grant in &snapshot.grants {
                if grant.session_id == session.id
                    && grant.grantee == context.contributor_id
                    && grant.status == SessionGrantStatus::Active
                    && grant
                        .expires_at_ms
                        .is_none_or(|expiry| self.now_ms < expiry)
                {
                    for permission in &grant.permissions {
                        if !permissions.contains(permission) {
                            permissions.push(*permission);
                        }
                    }
                }
            }
        }
        permissions
    }

    pub(super) fn visible(&self, id: &str) -> bool {
        self.session(id)
            .is_some_and(|session| !self.permissions(session).is_empty())
    }

    pub(super) fn can_observe(&self, id: &str) -> bool {
        self.session(id).is_some_and(|session| {
            self.permissions(session)
                .contains(&SessionPermission::Observe)
        })
    }

    pub(super) fn session(&self, id: &str) -> Option<&SessionInfo> {
        self.snapshot
            .as_ref()?
            .sessions
            .iter()
            .find(|session| session.id == id)
    }

    pub(super) fn actions(&self, session: &SessionInfo) -> Vec<SessionPermission> {
        if !self.ready() {
            return Vec::new();
        }
        let Some(snapshot) = &self.snapshot else {
            return Vec::new();
        };
        self.permissions(session)
            .into_iter()
            .filter(|permission| match permission {
                SessionPermission::Observe => true,
                SessionPermission::SubmitInput => {
                    snapshot.capabilities.input == SessionCapability::Available
                }
                SessionPermission::Invite => {
                    snapshot.capabilities.share == SessionCapability::Available
                }
            })
            .collect()
    }

    pub(super) fn prune(&mut self) {
        let visible: Vec<_> = self.snapshot.as_ref().map_or_else(Vec::new, |snapshot| {
            snapshot
                .sessions
                .iter()
                .filter(|session| self.visible(&session.id))
                .map(|session| session.id.clone())
                .collect()
        });
        let observable: Vec<_> = visible
            .iter()
            .filter(|id| self.can_observe(id))
            .cloned()
            .collect();
        self.prompts.retain(|prompt| {
            visible.contains(&prompt.input.session_id)
                && (observable.contains(&prompt.input.session_id) || prompt.text.is_some())
        });
        if self
            .selected
            .as_ref()
            .is_some_and(|id| !visible.contains(id))
        {
            self.selected = None;
        }
    }

    pub(super) fn view(&self) -> SessionViewModel {
        let sessions: Vec<_> = self.snapshot.as_ref().map_or_else(Vec::new, |snapshot| {
            snapshot
                .sessions
                .iter()
                .filter(|session| self.visible(&session.id))
                .map(|session| SessionView {
                    session: session.clone(),
                    relationship: if self
                        .context
                        .as_ref()
                        .is_some_and(|context| context.contributor_id == session.owner)
                    {
                        SessionRelationship::Owned
                    } else {
                        SessionRelationship::Invited
                    },
                    actions: self.actions(session),
                    grants: snapshot
                        .grants
                        .iter()
                        .filter(|grant| grant.session_id == session.id)
                        .cloned()
                        .collect(),
                })
                .collect()
        });
        let selected_history = self.selected.as_ref().and_then(|id| {
            sessions
                .iter()
                .find(|view| view.session.id == *id)
                .map(|view| view.session.history.clone())
        });
        let mutations: Vec<_> = self
            .mutations
            .iter()
            .filter(|mutation| {
                mutation
                    .mutation
                    .session_id()
                    .is_none_or(|id| self.visible(id))
            })
            .cloned()
            .collect();
        let prompts = self
            .prompts
            .iter()
            .filter(|prompt| {
                self.visible(&prompt.input.session_id)
                    && (self.can_observe(&prompt.input.session_id) || prompt.text.is_some())
            })
            .map(|prompt| {
                let mut prompt = prompt.clone();
                if !self.can_observe(&prompt.input.session_id) {
                    prompt.runtime = None;
                }
                prompt.submission = self
                    .mutations
                    .iter()
                    .find(|mutation| mutation.request.key() == prompt.input.request)
                    .map(|mutation| mutation.state.clone());
                prompt
            })
            .collect();
        SessionViewModel {
            context: self.context.clone(),
            load: self.load.clone(),
            updates: self.updates.clone(),
            capabilities: if self.ready() {
                self.snapshot
                    .as_ref()
                    .map_or_else(SessionCapabilities::default, |snapshot| {
                        snapshot.capabilities.clone()
                    })
            } else {
                SessionCapabilities::default()
            },
            sessions,
            selected: self.selected.clone(),
            selected_history,
            prompts,
            mutations,
            action_error: self.action_error.clone(),
        }
    }
}
