//! Pure editor transitions; asynchronous writes preserve their original payload.

use crux_core::{App, Command, render};
use serde::{Deserialize, Serialize};

use super::{
    ConfigurationAction, ConfigurationContext, ConfigurationDocument, ConfigurationDraft,
    ConfigurationError, ConfigurationErrorKind, ConfigurationLoadState, ConfigurationOperation,
    ConfigurationOutput, ConfigurationRequest, ConfigurationResult, ConfigurationSave,
    ConfigurationSaveState, ConfigurationSnapshot, ConfigurationViewModel, Model, validation,
};
use crate::Effect;

type ConfigurationCommand = Command<Effect, ConfigurationEvent>;

#[derive(Clone, Copy)]
enum DraftResolution {
    Discard,
    Rebase(Option<u64>),
}

/// Client intents, with internal completions excluded from the shell input codec.
#[derive(Clone, Debug, Deserialize, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ConfigurationEvent {
    /// Select an authenticated context and load both documents.
    Connect(ConfigurationContext),
    /// Reload both documents while preserving drafts and unresolved saves.
    Refresh,
    /// Replace a document's draft text, including invalid intermediate JSON.
    Edit {
        /// Settings or rules editor.
        document: ConfigurationDocument,
        /// Full JSON text; all unknown fields remain intact.
        json: String,
    },
    /// Discard a draft in favor of the latest confirmed value, unless saving.
    Discard(ConfigurationDocument),
    /// Retain the draft against the exact revision reviewed in a conflict.
    Rebase {
        /// Settings or rules editor.
        document: ConfigurationDocument,
        /// Revision shown alongside the draft; None means confirmed absence.
        reviewed_revision: Option<u64>,
    },
    /// Submit a changed, valid draft with a new host-persisted identity.
    Save {
        /// Editor to save; the other editor remains independent.
        document: ConfigurationDocument,
        /// New retry identity and original first-receipt deadline.
        request: ConfigurationRequest,
    },
    /// Recover an uncertain save by resending its exact original request.
    RetrySave(ConfigurationDocument),
    /// Stop effects after loss of delivery continuity, retaining both drafts.
    Suspend,
    /// Reload the same context before offering save or retry actions.
    Reconnect,
    /// Retire all continuations and clear this context's data.
    Disconnect,
    /// Restore a host-retained draft after loading its current authorized document.
    /// Preserve newer edits while recovering any original unresolved save.
    Restore(ConfigurationDraft),
    /// Internal continuation; serialized client events cannot forge saved values.
    #[serde(skip)]
    #[facet(skip)]
    Completed {
        /// Non-reusable local continuation identity.
        token: u64,
        /// Provider result for that exact operation.
        result: ConfigurationOutput,
    },
}

/// Settings/rules state only; no configuration persistence or policy enforcement.
#[derive(Debug, Default)]
pub struct Configuration;

impl App for Configuration {
    type Event = ConfigurationEvent;
    type Model = Model;
    type ViewModel = ConfigurationViewModel;
    type Effect = Effect;

    fn update(&self, event: ConfigurationEvent, model: &mut Model) -> ConfigurationCommand {
        match event {
            ConfigurationEvent::Connect(context) => {
                if let Err(error) = validation::context(&context) {
                    return action_error(model, error);
                }
                if model.context() == Some(&context) {
                    return Command::done();
                }
                model.reset();
                model.context = Some(context);
                refresh(model)
            }
            ConfigurationEvent::Refresh => refresh(model),
            ConfigurationEvent::Edit { document, json } => {
                let editor = model.editor_mut(document);
                if !editor.can_edit() {
                    return action_error(
                        model,
                        validation::error(
                            ConfigurationErrorKind::Forbidden,
                            "This configuration is read-only",
                        ),
                    );
                }
                editor.draft.json = json;
                model.action_error = None;
                render::render()
            }
            ConfigurationEvent::Discard(document) => {
                resolve_draft(model, document, DraftResolution::Discard)
            }
            ConfigurationEvent::Rebase {
                document,
                reviewed_revision,
            } => resolve_draft(model, document, DraftResolution::Rebase(reviewed_revision)),
            ConfigurationEvent::Save { document, request } => save(model, document, request),
            ConfigurationEvent::RetrySave(document) => {
                if !model.editor(document).can_retry() {
                    return Command::done();
                }
                let Some(save) = model.editor(document).pending.clone() else {
                    return Command::done();
                };
                submit_save(model, document, save)
            }
            ConfigurationEvent::Suspend => {
                if model.context.is_none() {
                    return Command::done();
                }
                model.suspend();
                render::render()
            }
            ConfigurationEvent::Reconnect => {
                if model.context.is_none() {
                    return Command::done();
                }
                model.suspend();
                model.settings.load = ConfigurationLoadState::Idle;
                model.agent_rules.load = ConfigurationLoadState::Idle;
                refresh(model)
            }
            ConfigurationEvent::Disconnect => {
                model.reset();
                render::render()
            }
            ConfigurationEvent::Restore(draft) => restore(model, draft),
            ConfigurationEvent::Completed { token, result } => complete(model, token, result),
        }
    }

    fn view(&self, model: &Model) -> ConfigurationViewModel {
        model.view()
    }
}

fn restore(model: &mut Model, draft: ConfigurationDraft) -> ConfigurationCommand {
    if model.context() != Some(&draft.context) {
        return action_error(
            model,
            validation::invalid("Saved draft belongs to another configuration context"),
        );
    }
    let editor = model.editor(draft.document);
    let keep_edits = editor.dirty();
    if editor.pending.is_some() || (keep_edits && draft.pending.is_none()) {
        return Command::done();
    }
    if !editor.is_ready()
        || draft.value.schema_version != super::DOCUMENT_VERSION
        || draft.value.json.len() > 1024 * 1024
        || draft
            .base
            .as_ref()
            .is_some_and(|base| base.revision == 0 || validation::value(&base.value).is_err())
        || draft.pending.as_ref().is_some_and(|save| {
            save.request.request_id.trim().is_empty()
                || save.request.request_id.len() > 1024
                || save.request.expires_at_ms == 0
                || validation::value(&save.value).is_err()
                || save.expected_revision != draft.base.as_ref().map(|base| base.revision)
        })
    {
        return action_error(
            model,
            validation::invalid(
                "Saved configuration draft is invalid or its document is not loaded",
            ),
        );
    }
    if let Some(save) = &draft.pending
        && !model.used_requests.insert(save.request.request_id.clone())
    {
        return action_error(
            model,
            validation::invalid("Saved request identity is already in use"),
        );
    }
    let editor = model.editor_mut(draft.document);
    editor.base = draft.base;
    if !keep_edits {
        editor.draft = draft.value;
    }
    editor.save = if draft.pending.is_some() {
        ConfigurationSaveState::Uncertain(validation::error(
            ConfigurationErrorKind::Unavailable,
            "Restored save has an unknown outcome",
        ))
    } else {
        ConfigurationSaveState::Idle
    };
    editor.pending = draft.pending;
    model.action_error = None;
    render::render()
}

fn action_error(model: &mut Model, error: ConfigurationError) -> ConfigurationCommand {
    model.action_error = Some(error);
    render::render()
}

fn resolve_draft(
    model: &mut Model,
    document: ConfigurationDocument,
    resolution: DraftResolution,
) -> ConfigurationCommand {
    let editor = model.editor_mut(document);
    if editor.pending.is_some() || !editor.is_ready() {
        return action_error(
            model,
            validation::error(
                ConfigurationErrorKind::InvalidInput,
                "Wait for the current load or save outcome before resolving this draft",
            ),
        );
    }
    if let DraftResolution::Rebase(reviewed_revision) = resolution {
        if !editor.can_edit() {
            return action_error(
                model,
                validation::error(
                    ConfigurationErrorKind::Forbidden,
                    "This configuration is read-only",
                ),
            );
        }
        let current = editor
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.record.as_ref());
        if current.map(|record| record.revision) != reviewed_revision {
            return action_error(
                model,
                validation::error(
                    ConfigurationErrorKind::Conflict,
                    "Configuration changed since review; review the current revision before rebasing",
                ),
            );
        }
        editor.base = current.cloned();
        editor.save = ConfigurationSaveState::Idle;
    } else {
        editor.discard();
    }
    model.action_error = None;
    render::render()
}

fn refresh(model: &mut Model) -> ConfigurationCommand {
    reload(model, ConfigurationDocument::Settings)
        .and(reload(model, ConfigurationDocument::AgentRules))
}

fn reload(model: &mut Model, document: ConfigurationDocument) -> ConfigurationCommand {
    if model.context.is_none() {
        return Command::done();
    }
    let editor = model.editor_mut(document);
    if editor.load == ConfigurationLoadState::Suspended {
        return Command::done();
    }
    if matches!(
        editor.load,
        ConfigurationLoadState::Loading | ConfigurationLoadState::Refreshing
    ) || editor.save == ConfigurationSaveState::Saving
    {
        editor.refresh_again = true;
        return Command::done();
    }
    editor.refresh_again = false;
    editor.load = if editor.is_ready() {
        ConfigurationLoadState::Refreshing
    } else {
        ConfigurationLoadState::Loading
    };
    dispatch(model, document, ConfigurationAction::Load)
}

fn dispatch(
    model: &mut Model,
    document: ConfigurationDocument,
    action: ConfigurationAction,
) -> ConfigurationCommand {
    let Some(context) = model.context.clone() else {
        return Command::done();
    };
    let operation = ConfigurationOperation {
        context,
        document,
        action,
    };
    let Ok(token) = model.requests.register(operation.clone(), false) else {
        failed(
            model,
            &operation,
            validation::invalid("Configuration continuation identities exhausted"),
        );
        return render::render();
    };
    Command::request_from_shell(operation)
        .then_send(move |result| ConfigurationEvent::Completed { token, result })
        .and(render::render())
}

fn save(
    model: &mut Model,
    document: ConfigurationDocument,
    request: ConfigurationRequest,
) -> ConfigurationCommand {
    if !model.editor(document).can_save() {
        return action_error(
            model,
            validation::error(
                ConfigurationErrorKind::InvalidInput,
                "Load and validate a changed draft, then resolve conflicts before saving",
            ),
        );
    }
    if request.request_id.trim().is_empty()
        || request.expires_at_ms == 0
        || !model.used_requests.insert(request.request_id.clone())
    {
        return action_error(
            model,
            validation::error(
                ConfigurationErrorKind::InvalidInput,
                "A save requires a new nonempty request identity and deadline",
            ),
        );
    }
    let editor = model.editor_mut(document);
    let save = ConfigurationSave {
        request,
        expected_revision: editor.base.as_ref().map(|base| base.revision),
        value: editor.draft.clone(),
    };
    submit_save(model, document, save)
}

fn submit_save(
    model: &mut Model,
    document: ConfigurationDocument,
    save: ConfigurationSave,
) -> ConfigurationCommand {
    // A read started before this save cannot update state after its outcome.
    model.requests.retain(|operation| {
        operation.document != document || operation.action != ConfigurationAction::Load
    });
    let editor = model.editor_mut(document);
    if editor.load == ConfigurationLoadState::Refreshing {
        editor.load = ConfigurationLoadState::Ready;
        editor.refresh_again = true;
    }
    editor.pending = Some(save.clone());
    editor.save = ConfigurationSaveState::Saving;
    model.action_error = None;
    dispatch(model, document, ConfigurationAction::Save(save))
}

fn complete(model: &mut Model, token: u64, result: ConfigurationOutput) -> ConfigurationCommand {
    let Some((operation, _window)) = model.requests.take(token) else {
        return Command::done();
    };
    let outcome = result.and_then(|result| apply_result(model, &operation, result));
    if let Err(error) = outcome {
        failed(model, &operation, error);
    }
    let command = render::render();
    if model.editor(operation.document).refresh_again {
        command.and(reload(model, operation.document))
    } else {
        command
    }
}

fn failed(model: &mut Model, operation: &ConfigurationOperation, error: ConfigurationError) {
    let editor = model.editor_mut(operation.document);
    match &operation.action {
        ConfigurationAction::Load => editor.load = ConfigurationLoadState::Failed(error),
        ConfigurationAction::Save(_) => editor.save = ConfigurationSaveState::Uncertain(error),
    }
}

fn apply_result(
    model: &mut Model,
    operation: &ConfigurationOperation,
    result: ConfigurationResult,
) -> Result<(), ConfigurationError> {
    match (&operation.action, result) {
        (ConfigurationAction::Load, ConfigurationResult::Loaded(snapshot)) => {
            validation::snapshot(&snapshot, operation)?;
            let editor = model.editor_mut(operation.document);
            validation::progression(
                editor
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.record.as_ref()),
                snapshot.record.as_ref(),
            )?;
            editor.adopt(snapshot);
            Ok(())
        }
        (ConfigurationAction::Save(save), ConfigurationResult::Saved { request, snapshot })
            if request == save.request =>
        {
            saved(model, operation, save, snapshot)
        }
        (ConfigurationAction::Save(save), ConfigurationResult::Rejected { request, error })
            if request == save.request =>
        {
            let editor = model.editor_mut(operation.document);
            if matches!(
                error.kind,
                ConfigurationErrorKind::Conflict
                    | ConfigurationErrorKind::Forbidden
                    | ConfigurationErrorKind::Unauthenticated
            ) {
                // Reload the revision/capability before allowing another write.
                editor.load = ConfigurationLoadState::Failed(error.clone());
                editor.refresh_again = true;
            }
            editor.pending = None;
            editor.save = ConfigurationSaveState::Failed(error);
            Ok(())
        }
        (
            ConfigurationAction::Load | ConfigurationAction::Save(_),
            ConfigurationResult::Loaded(_)
            | ConfigurationResult::Saved { .. }
            | ConfigurationResult::Rejected { .. },
        ) => Err(validation::invalid(
            "Configuration result kind or retry identity does not match its operation",
        )),
    }
}

fn saved(
    model: &mut Model,
    operation: &ConfigurationOperation,
    save: &ConfigurationSave,
    snapshot: ConfigurationSnapshot,
) -> Result<(), ConfigurationError> {
    validation::snapshot(&snapshot, operation)?;
    let Some(record) = &snapshot.record else {
        return Err(validation::invalid(
            "A committed configuration must have a saved record",
        ));
    };
    if record.revision <= save.expected_revision.unwrap_or(0) || record.value != save.value {
        return Err(validation::invalid(
            "Committed configuration does not match the submitted value and revision precondition",
        ));
    }
    let editor = model.editor_mut(operation.document);
    let current = editor
        .snapshot
        .as_ref()
        .and_then(|snapshot| snapshot.record.as_ref());
    let already_known = current.is_some_and(|current| current.revision >= record.revision);
    if current.is_none_or(|current| current.revision <= record.revision) {
        validation::progression(current, Some(record))?;
    }
    let edited_during_save = editor.draft != save.value;
    editor.base = Some(record.clone());
    editor.pending = None;
    editor.save = ConfigurationSaveState::Saved(record.revision);
    if !already_known {
        editor.snapshot = Some(snapshot);
    }
    if !edited_during_save {
        editor.base = editor
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.record.clone());
        editor.draft = editor
            .base
            .as_ref()
            .map_or_else(super::ConfigurationValue::default, |base| {
                base.value.clone()
            });
    }
    Ok(())
}
