//! Saved values and drafts remain separate throughout asynchronous work.

use std::collections::BTreeSet;

use idle_history::requests::RequestTracker;

use super::{
    ConfigurationContext, ConfigurationDocument, ConfigurationEditorAction,
    ConfigurationEditorView, ConfigurationError, ConfigurationErrorKind, ConfigurationLoadState,
    ConfigurationOperation, ConfigurationRecord, ConfigurationSave, ConfigurationSaveState,
    ConfigurationSnapshot, ConfigurationValue, ConfigurationViewModel, validation,
};

#[derive(Debug, Default)]
pub(super) struct Editor {
    pub snapshot: Option<ConfigurationSnapshot>,
    pub base: Option<ConfigurationRecord>,
    pub draft: ConfigurationValue,
    pub load: ConfigurationLoadState,
    pub save: ConfigurationSaveState,
    pub pending: Option<ConfigurationSave>,
    pub refresh_again: bool,
}

impl Editor {
    pub(super) fn dirty(&self) -> bool {
        self.base.as_ref().map_or_else(
            || self.draft != ConfigurationValue::default(),
            |base| self.draft != base.value,
        )
    }

    pub(super) fn conflict(&self) -> bool {
        self.snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.record != self.base)
    }

    pub(super) fn can_edit(&self) -> bool {
        self.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot.can_edit
                && snapshot
                    .record
                    .as_ref()
                    .is_none_or(|record| record.value.schema_version == super::DOCUMENT_VERSION)
        })
    }

    pub(super) fn is_ready(&self) -> bool {
        matches!(
            self.load,
            ConfigurationLoadState::Ready | ConfigurationLoadState::Refreshing
        )
    }

    pub(super) fn can_save(&self) -> bool {
        self.is_ready()
            && self.can_edit()
            && self.dirty()
            && !self.conflict()
            && self.pending.is_none()
            && validation::value(&self.draft).is_ok()
    }

    pub(super) fn can_retry(&self) -> bool {
        self.is_ready()
            && self.pending.is_some()
            && matches!(self.save, ConfigurationSaveState::Uncertain(_))
    }

    pub(super) fn adopt(&mut self, snapshot: ConfigurationSnapshot) {
        if !self.dirty() && self.pending.is_none() {
            self.base.clone_from(&snapshot.record);
            self.draft = snapshot
                .record
                .as_ref()
                .map_or_else(ConfigurationValue::default, |record| record.value.clone());
        }
        self.snapshot = Some(snapshot);
        self.load = ConfigurationLoadState::Ready;
    }

    pub(super) fn discard(&mut self) {
        self.base = self
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.record.clone());
        self.draft = self
            .base
            .as_ref()
            .map_or_else(ConfigurationValue::default, |base| base.value.clone());
        self.save = ConfigurationSaveState::Idle;
    }

    pub(super) fn suspend(&mut self) {
        self.load = ConfigurationLoadState::Suspended;
        self.refresh_again = false;
        if self.pending.is_some() {
            self.save = ConfigurationSaveState::Uncertain(validation::error(
                ConfigurationErrorKind::Unavailable,
                "Connection interrupted; retry the original save to recover its result",
            ));
        }
    }

    pub(super) fn view(&self) -> ConfigurationEditorView {
        let mut actions = Vec::new();
        if self.can_edit() {
            actions.push(ConfigurationEditorAction::Edit);
        }
        if self.can_save() {
            actions.push(ConfigurationEditorAction::Save);
        }
        if self.can_retry() {
            actions.push(ConfigurationEditorAction::RetrySave);
        }
        if self.pending.is_none() && self.is_ready() {
            actions.push(ConfigurationEditorAction::Discard);
            if self.conflict() && self.can_edit() {
                actions.push(ConfigurationEditorAction::Rebase);
            }
        }
        ConfigurationEditorView {
            current: self
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.record.clone()),
            base_revision: self.base.as_ref().map(|base| base.revision),
            draft: self.draft.clone(),
            dirty: self.dirty(),
            conflict: self.conflict(),
            validation_error: validation::value(&self.draft).err(),
            load: self.load.clone(),
            save: self.save.clone(),
            pending: self.pending.clone(),
            actions,
        }
    }
}

/// One client's independent settings and rules editors and continuation ownership.
#[derive(Debug, Default)]
pub struct Model {
    pub(super) context: Option<ConfigurationContext>,
    pub(super) settings: Editor,
    pub(super) agent_rules: Editor,
    pub(super) requests: RequestTracker<ConfigurationOperation>,
    pub(super) used_requests: BTreeSet<String>,
    pub(super) action_error: Option<ConfigurationError>,
}

impl Model {
    /// Selected authenticated configuration context.
    #[must_use]
    pub fn context(&self) -> Option<&ConfigurationContext> {
        self.context.as_ref()
    }

    pub(super) fn editor(&self, document: ConfigurationDocument) -> &Editor {
        match document {
            ConfigurationDocument::Settings => &self.settings,
            ConfigurationDocument::AgentRules => &self.agent_rules,
        }
    }

    pub(super) fn editor_mut(&mut self, document: ConfigurationDocument) -> &mut Editor {
        match document {
            ConfigurationDocument::Settings => &mut self.settings,
            ConfigurationDocument::AgentRules => &mut self.agent_rules,
        }
    }

    pub(super) fn reset(&mut self) {
        self.requests.clear();
        self.context = None;
        self.settings = Editor::default();
        self.agent_rules = Editor::default();
        self.used_requests.clear();
        self.action_error = None;
    }

    pub(super) fn suspend(&mut self) {
        self.requests.clear();
        self.settings.suspend();
        self.agent_rules.suspend();
    }

    pub(crate) fn wait_for_connection(&mut self, context: ConfigurationContext) {
        if let Err(error) = validation::context(&context) {
            self.action_error = Some(error);
            return;
        }
        if self.context.as_ref() != Some(&context) {
            self.reset();
            self.context = Some(context);
        }
        self.suspend();
    }

    pub(super) fn view(&self) -> ConfigurationViewModel {
        ConfigurationViewModel {
            context: self.context.clone(),
            settings: self.settings.view(),
            agent_rules: self.agent_rules.view(),
            action_error: self.action_error.clone(),
        }
    }
}
