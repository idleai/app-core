use crux_core::{App, Command, render};
use serde::{Deserialize, Serialize};

use super::{
    Model, RepositoryAction, RepositoryContext, RepositoryLoadState, RepositoryOutput,
    RepositoryQuery, RepositoryResult, SourceRecord, ViewModel,
};
use crate::{effects::Effect, history, module::EffectError};

type RepositoryCommand = Command<Effect, RepositoryEvent>;

/// Repository read, account connection and recorded-session selection intents.
#[derive(Clone, Debug, Deserialize, Serialize, facet::Facet)]
#[repr(u8)]
pub enum RepositoryEvent {
    /// Bind the authorized repository and read one replacement.
    Connect(RepositoryContext),
    /// Read all sources again, coalescing changes during a pending read.
    Refresh,
    /// Local history changed; reuse recently checked GitHub data when possible.
    Changed,
    /// Explicitly connect a GitHub account through the host.
    SignIn,
    /// Select a recorded session's exact history, or clear the session filter.
    SelectSession(Option<String>),
    /// Inspect an exact record actually supplied for this session.
    Inspect {
        /// Full logical session identity.
        session: String,
        /// Exact supplied source address.
        record: SourceRecord,
    },
    /// Retire pending reads after delivery continuity is lost.
    Suspend,
    /// Resume by replacing all repository data.
    Reconnect,
    /// Clear data and retire every previous continuation.
    Disconnect,
    /// Internal host continuation, excluded from serialized client actions.
    #[serde(skip)]
    #[facet(skip)]
    Completed {
        /// Non-reusable local request identity.
        token: u64,
        /// Matching host result.
        result: RepositoryOutput,
    },
}

/// Shared reducer for Git/GitHub views and recorded-session history navigation.
#[derive(Debug, Default)]
pub struct Repositories;

impl App for Repositories {
    type Event = RepositoryEvent;
    type Model = Model;
    type ViewModel = ViewModel;
    type Effect = Effect;

    fn update(&self, event: RepositoryEvent, model: &mut Model) -> RepositoryCommand {
        match event {
            RepositoryEvent::Connect(context) => {
                if [
                    &context.repository_id,
                    &context.connection.provider,
                    &context.connection.workspace,
                    &context.connection.chain,
                    &context.connection.contributor,
                ]
                .iter()
                .any(|value| value.is_empty())
                {
                    return action_error(model, "Repository context is incomplete.");
                }
                if model.context.as_ref() == Some(&context) {
                    return Command::done();
                }
                model.reset();
                model.context = Some(context);
                refresh(model, RepositoryAction::Read)
            }
            RepositoryEvent::Refresh => refresh(model, RepositoryAction::Read),
            RepositoryEvent::Changed => refresh(model, RepositoryAction::Poll),
            RepositoryEvent::SignIn => {
                model.requests.clear();
                model.refresh_again = None;
                refresh(model, RepositoryAction::SignIn)
            }
            RepositoryEvent::SelectSession(id) => select(model, id),
            RepositoryEvent::Inspect { session, record } => {
                if !model.snapshot.as_ref().is_some_and(|snapshot| {
                    snapshot
                        .sessions
                        .iter()
                        .any(|item| item.id == session && item.records.contains(&record))
                }) {
                    return action_error(
                        model,
                        "This record is not supplied by the selected recorded session.",
                    );
                }
                let command = select(model, Some(session));
                model
                    .history
                    .push(history::Event::Select(history::Selected {
                        item: Some(record.item),
                        observation: Some(record.observation),
                    }));
                model.action_error = None;
                command
            }
            RepositoryEvent::Suspend => {
                if model.context.is_none() {
                    return Command::done();
                }
                model.requests.clear();
                model.refresh_again = None;
                model.stale = true;
                model.load = RepositoryLoadState::Suspended;
                render::render()
            }
            RepositoryEvent::Reconnect => {
                model.requests.clear();
                model.refresh_again = None;
                model.load = RepositoryLoadState::Idle;
                refresh(model, RepositoryAction::Read)
            }
            RepositoryEvent::Disconnect => {
                model.reset();
                render::render()
            }
            RepositoryEvent::Completed { token, result } => complete(model, token, result),
        }
    }

    fn view(&self, model: &Model) -> ViewModel {
        ViewModel {
            context: model.context.clone(),
            snapshot: model.snapshot.clone(),
            selected_session: model.selected.clone(),
            load: model.load.clone(),
            needs_refresh: model.stale,
            action_error: model.action_error.clone(),
        }
    }
}

fn refresh(model: &mut Model, action: RepositoryAction) -> RepositoryCommand {
    let Some(context) = model
        .context
        .clone()
        .filter(|_| model.load != RepositoryLoadState::Suspended)
    else {
        return Command::done();
    };
    if model.requests.pending_window().is_some() {
        if model.refresh_again != Some(RepositoryAction::Read) {
            model.refresh_again = Some(action);
        }
        return Command::done();
    }
    let query = RepositoryQuery { context, action };
    let Ok(token) = model.requests.register(query.clone(), true) else {
        return action_error(model, "Repository request identities exhausted.");
    };
    model.load = RepositoryLoadState::Loading;
    model.stale = true;
    model.action_error = None;
    Command::request_from_shell(query)
        .then_send(move |result| RepositoryEvent::Completed { token, result })
        .and(render::render())
}

fn complete(model: &mut Model, token: u64, result: RepositoryOutput) -> RepositoryCommand {
    let Some((query, _window)) = model.requests.take(token) else {
        return Command::done();
    };
    if model.context.as_ref() != Some(&query.context) {
        return Command::done();
    }
    if let RepositoryAction::Remember(selected) = query.action {
        if selected != model.selected {
            return Command::done();
        }
        return match result {
            Ok(RepositoryResult::Remembered) => render::render(),
            Ok(RepositoryResult::Snapshot { .. }) => {
                action_error(model, "Unexpected repository selection response.")
            }
            Err(error) => {
                model.action_error = Some(error);
                render::render()
            }
        };
    }
    let result = result.and_then(|result| {
        let RepositoryResult::Snapshot {
            snapshot,
            selected_session,
        } = result
        else {
            return Err(error("Expected a repository replacement."));
        };
        if snapshot.scope.workspace_id != query.context.connection.workspace
            || snapshot.scope.chain != query.context.connection.chain
            || snapshot.scope.repository_id != query.context.repository_id
        {
            return Err(error("Repository data belongs to a different binding."));
        }
        snapshot.validate().map_err(error)?;
        Ok((*snapshot, selected_session))
    });
    match result {
        Ok((snapshot, selected_session)) => {
            let complete_sessions = snapshot.reports.iter().any(|report| {
                report.topic == "history.sessions" && report.state == super::ReadState::Complete
            });
            let retained_selection = selected_session
                .filter(|id| snapshot.sessions.iter().any(|session| &session.id == id));
            if !model.selection_initialized && (complete_sessions || retained_selection.is_some()) {
                model.selection_initialized = true;
                model.selected = retained_selection;
                if let Some(id) = &model.selected {
                    model
                        .history
                        .push(history::Event::SetFilter(history::Filter {
                            session: Some(id.clone()),
                            ..history::Filter::default()
                        }));
                    model
                        .history
                        .push(history::Event::Select(history::Selected {
                            item: Some(id.clone()),
                            observation: None,
                        }));
                }
            }
            if complete_sessions
                && model
                    .selected
                    .as_ref()
                    .is_some_and(|id| !snapshot.sessions.iter().any(|session| &session.id == id))
            {
                model.selected = None;
                model
                    .history
                    .push(history::Event::SetFilter(history::Filter::default()));
            }
            model.snapshot = Some(snapshot);
            model.load = RepositoryLoadState::Ready;
            model.stale = false;
        }
        Err(error) => {
            model.load = RepositoryLoadState::Failed(error);
            model.stale = true;
        }
    }
    if let Some(action) = model.refresh_again.take() {
        return refresh(model, action);
    }
    render::render()
}

fn select(model: &mut Model, id: Option<String>) -> RepositoryCommand {
    let Some(context) = model
        .context
        .clone()
        .filter(|_| model.load != RepositoryLoadState::Suspended)
    else {
        return Command::done();
    };
    if id.as_ref().is_some_and(|id| {
        !model
            .snapshot
            .as_ref()
            .is_some_and(|snapshot| snapshot.sessions.iter().any(|session| &session.id == id))
    }) {
        return action_error(
            model,
            "Recorded session is not in the current repository input.",
        );
    }
    model.selected.clone_from(&id);
    model.selection_initialized = true;
    model.action_error = None;
    model
        .history
        .push(history::Event::SetFilter(history::Filter {
            session: id.clone(),
            ..history::Filter::default()
        }));
    if let Some(id) = &id {
        model
            .history
            .push(history::Event::Select(history::Selected {
                item: Some(id.clone()),
                observation: None,
            }));
    }
    model
        .requests
        .retain(|query| !matches!(query.action, RepositoryAction::Remember(_)));
    let query = RepositoryQuery {
        context,
        action: RepositoryAction::Remember(id),
    };
    let Ok(token) = model.requests.register(query.clone(), false) else {
        return action_error(model, "Repository request identities exhausted.");
    };
    Command::request_from_shell(query)
        .then_send(move |result| RepositoryEvent::Completed { token, result })
        .and(render::render())
}

fn error(message: impl Into<String>) -> EffectError {
    EffectError {
        message: message.into(),
    }
}
fn action_error(model: &mut Model, message: &str) -> RepositoryCommand {
    model.action_error = Some(error(message));
    render::render()
}
