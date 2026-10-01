//! Pure subscription transitions with correlated host callbacks.

use crux_core::{App, Command, render};
use serde::{Deserialize, Serialize};

use super::{
    Connection, ConnectionStatus, Context, RequestTracker, SubscriptionAction, SubscriptionError,
    SubscriptionErrorKind, SubscriptionOperation, SubscriptionOutput, SubscriptionResult,
    SubscriptionViewModel,
};
use crate::{effects::Effect, history};

type SubscriptionCommand = Command<Effect, Event>;
use SubscriptionEvent as Event;

/// One client's authorized connection lifetime and pending callbacks.
#[derive(Debug, Default)]
pub struct Model {
    context: Option<Context>,
    lifecycle: Connection,
    connection: Option<String>,
    error: Option<SubscriptionError>,
    pending: RequestTracker<SubscriptionOperation>,
    history: Option<history::Event>,
}

impl Model {
    /// Full current context, including provider and audience.
    #[must_use]
    pub const fn context(&self) -> Option<&Context> {
        self.context.as_ref()
    }

    pub(crate) fn has_connection(&self) -> bool {
        self.connection.is_some()
    }

    pub(crate) fn take_history_event(&mut self) -> Option<history::Event> {
        self.history.take()
    }

    pub(crate) fn history_state(&mut self, state: &history::RequestState) {
        if self.connection.is_none() {
            return;
        }
        let status = match state {
            history::RequestState::Ready => {
                self.error = None;
                ConnectionStatus::Live
            }
            history::RequestState::Loading => ConnectionStatus::Reconciling,
            history::RequestState::Failed(error) => {
                self.error = Some(SubscriptionError {
                    kind: SubscriptionErrorKind::Transport,
                    message: error.message.clone(),
                });
                ConnectionStatus::Failed
            }
            history::RequestState::Idle => return,
        };
        let _accepted = self.lifecycle.update(self.lifecycle.generation(), status);
    }
}

/// Client actions. Provider results cannot be forged through serialized events.
#[derive(Clone, Debug, Deserialize, Serialize, facet::Facet)]
#[repr(u8)]
pub enum SubscriptionEvent {
    /// Join a provider/audience/chain context; changing any component retires old work.
    Connect(Context),
    /// Rejoin the current context and reconcile from a fresh snapshot.
    Reconnect,
    /// Stop joining and release the connection.
    Disconnect,
    /// Internal typed host continuation.
    #[serde(skip)]
    #[facet(skip)]
    Completed {
        /// Monotonic continuation identity.
        request: u64,
        /// Captured operation permits cleanup of a join that finished after retirement.
        operation: SubscriptionOperation,
        /// Matching host response.
        result: SubscriptionOutput,
    },
}

/// Shared Crux subscription reducer.
#[derive(Debug, Default)]
pub struct Subscriptions;

impl App for Subscriptions {
    type Event = Event;
    type Model = Model;
    type ViewModel = SubscriptionViewModel;
    type Effect = Effect;

    fn update(&self, event: Event, model: &mut Model) -> SubscriptionCommand {
        match event {
            Event::Connect(context) => {
                if model.context.as_ref() == Some(&context) {
                    return Command::done();
                }
                let command = retire(model);
                model.context = Some(context);
                command.and(join(model))
            }
            Event::Reconnect => {
                let command = retire(model);
                command.and(join(model))
            }
            Event::Disconnect => {
                let command = retire(model);
                model.context = None;
                model.error = None;
                command.and(render::render())
            }
            Event::Completed {
                request,
                operation,
                result,
            } => {
                if model.pending.take(request).is_none() {
                    return match result {
                        Ok(SubscriptionResult::Joined { connection })
                            if matches!(operation.action, SubscriptionAction::Join) =>
                        {
                            leave(operation.context, connection)
                        }
                        Ok(
                            SubscriptionResult::Joined { .. }
                            | SubscriptionResult::Changed
                            | SubscriptionResult::Closed
                            | SubscriptionResult::Elapsed
                            | SubscriptionResult::Left,
                        )
                        | Err(_) => Command::done(),
                    };
                }
                complete(model, operation.action, result)
            }
        }
    }

    fn view(&self, model: &Model) -> SubscriptionViewModel {
        SubscriptionViewModel {
            context: model.context.clone(),
            status: model.lifecycle.status(),
            error: model.error.clone(),
        }
    }
}

fn retire(model: &mut Model) -> SubscriptionCommand {
    model.pending.clear();
    model.lifecycle.stop();
    model.history = Some(history::Event::Suspend);
    match (model.context.clone(), model.connection.take()) {
        (Some(context), Some(connection)) => leave(context, connection),
        _ => Command::done(),
    }
}

fn leave(context: Context, connection: String) -> SubscriptionCommand {
    let operation = SubscriptionOperation {
        context,
        action: SubscriptionAction::Leave { connection },
    };
    // Cleanup cannot update a later context. Its completion has no request owner.
    Command::request_from_shell(operation.clone()).then_send(move |result| Event::Completed {
        request: 0,
        operation: operation.clone(),
        result,
    })
}

fn join(model: &mut Model) -> SubscriptionCommand {
    let Some(context) = &model.context else {
        return render::render();
    };
    if [
        &context.provider,
        &context.workspace,
        &context.contributor,
        &context.chain,
    ]
    .iter()
    .any(|part| part.is_empty())
    {
        model.error = Some(invalid(
            "Subscription context must include provider, workspace, contributor and chain",
        ));
        return render::render();
    }
    if model.lifecycle.begin().is_none() {
        model.error = Some(invalid("Subscription generations exhausted"));
        return render::render();
    }
    model.error = None;
    issue(model, SubscriptionAction::Join)
}

fn issue(model: &mut Model, action: SubscriptionAction) -> SubscriptionCommand {
    let Some(context) = model.context.clone() else {
        return Command::done();
    };
    let operation = SubscriptionOperation { context, action };
    let Ok(request) = model.pending.register(operation.clone(), false) else {
        model.error = Some(invalid("Subscription request identities exhausted"));
        let _accepted = model
            .lifecycle
            .update(model.lifecycle.generation(), ConnectionStatus::Failed);
        return render::render();
    };
    Command::request_from_shell(operation.clone())
        .then_send(move |result| Event::Completed {
            request,
            operation: operation.clone(),
            result,
        })
        .and(render::render())
}

fn complete(
    model: &mut Model,
    action: SubscriptionAction,
    result: SubscriptionOutput,
) -> SubscriptionCommand {
    match (action, result) {
        (SubscriptionAction::Join, Ok(SubscriptionResult::Joined { connection }))
            if !connection.is_empty() =>
        {
            model.connection = Some(connection.clone());
            let _accepted = model
                .lifecycle
                .update(model.lifecycle.generation(), ConnectionStatus::Reconciling);
            model.history = Some(history::Event::Reconnect);
            issue(model, SubscriptionAction::Watch { connection })
        }
        (SubscriptionAction::Watch { connection }, Ok(SubscriptionResult::Changed)) => {
            let _accepted = model
                .lifecycle
                .update(model.lifecycle.generation(), ConnectionStatus::Reconciling);
            model.history = Some(history::Event::Refresh);
            issue(model, SubscriptionAction::Watch { connection })
        }
        (SubscriptionAction::Watch { .. }, Ok(SubscriptionResult::Closed)) => fail(
            model,
            SubscriptionError {
                kind: SubscriptionErrorKind::Transport,
                message: "Connection closed; waiting to reconnect".into(),
            },
        ),
        (SubscriptionAction::Wait { .. }, Ok(SubscriptionResult::Elapsed)) => join(model),
        (_, Err(error)) => fail(model, error),
        (
            _,
            Ok(
                SubscriptionResult::Joined { .. }
                | SubscriptionResult::Changed
                | SubscriptionResult::Closed
                | SubscriptionResult::Elapsed
                | SubscriptionResult::Left,
            ),
        ) => fail(
            model,
            invalid("Subscription response does not match its request"),
        ),
    }
}

fn fail(model: &mut Model, error: SubscriptionError) -> SubscriptionCommand {
    model.pending.clear();
    model.history = Some(history::Event::Suspend);
    let mut command = match (model.context.clone(), model.connection.take()) {
        (Some(context), Some(connection)) => leave(context, connection),
        _ => Command::done(),
    };
    match error.kind {
        SubscriptionErrorKind::Transport => {
            let _accepted = model
                .lifecycle
                .update(model.lifecycle.generation(), ConnectionStatus::Waiting);
            command = command.and(issue(
                model,
                SubscriptionAction::Wait {
                    delay_ms: model.lifecycle.retry_delay_ms(),
                },
            ));
        }
        SubscriptionErrorKind::Unavailable => {
            let _accepted = model
                .lifecycle
                .update(model.lifecycle.generation(), ConnectionStatus::Failed);
        }
        SubscriptionErrorKind::Unauthorized => {
            let _accepted = model
                .lifecycle
                .update(model.lifecycle.generation(), ConnectionStatus::Expired);
            model.context = None;
        }
    }
    model.error = Some(error);
    command.and(render::render())
}

fn invalid(message: &str) -> SubscriptionError {
    SubscriptionError {
        kind: SubscriptionErrorKind::Unavailable,
        message: message.into(),
    }
}
