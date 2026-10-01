//! Session actions and context-correlated Crux continuations.

use crux_core::{App, Command, render};
use serde::{Deserialize, Serialize};

use super::{
    Model, SessionAction, SessionContext, SessionDraft, SessionError, SessionErrorCode,
    SessionInputRef, SessionLoadState, SessionMutation, SessionMutationId, SessionOperation,
    SessionOutput, SessionPermission, SessionViewModel, model::State, mutations, recovery,
    validation,
};
use crate::effects::Effect;

pub(super) type SessionCommand = Command<Effect, SessionEvent>;

/// Client intents; runtime and coordination facts enter through host results only.
#[derive(Clone, Debug, Deserialize, Serialize, facet::Facet)]
#[repr(u8)]
pub enum SessionEvent {
    /// Connect an explicit provider, audience and workspace/chain binding.
    Connect(SessionContext),
    /// Reconcile from a fresh snapshot, preserving same-context selection/input.
    Refresh,
    /// Select an owned/invited session, or clear selection.
    Select(Option<String>),
    /// Create a runner through the host's connected runtime adapter.
    Create {
        /// Stable caller-generated retry identity.
        id: SessionMutationId,
        /// Requested runtime creation settings.
        draft: SessionDraft,
    },
    /// Submit exact text to the selected session as the authenticated contributor.
    Submit {
        /// Stable caller-generated retry identity.
        id: SessionMutationId,
        /// Exact prompt text, retained while pending.
        text: String,
    },
    /// Share the selected session using an explicit participation grant.
    Invite {
        /// Stable caller-generated retry identity.
        id: SessionMutationId,
        /// New grant identity.
        grant_id: String,
        /// Contributor being invited.
        grantee: String,
        /// Session actions to share, without compute access.
        permissions: Vec<SessionPermission>,
        /// Optional exclusive grant deadline.
        expires_at_ms: Option<u64>,
    },
    /// Revoke an existing grant on the selected session.
    Revoke {
        /// Stable caller-generated retry identity.
        id: SessionMutationId,
        /// Existing grant identity; the reducer supplies its known revision.
        grant_id: String,
    },
    /// Retry an original mutation only when its returned advice permits it.
    Retry(String),
    /// Query an uncertain mutation using its original request key.
    Recover(String),
    /// Ask for the current runtime fact for a known visible input.
    RefreshInput(SessionInputRef),
    /// Advance the provider-aligned clock for grant expiry and retry deadlines.
    Tick(u64),
    /// Detach this client's state; never stop a session or cancel its execution.
    Disconnect,
    /// Internal completion; serialized client events cannot forge runtime facts.
    #[serde(skip)]
    #[facet(skip)]
    Completed {
        /// Monotonic client-local continuation token.
        token: u64,
        /// Authenticated host result for that pending operation.
        result: SessionOutput,
    },
}

/// Shared reducer for standalone and managed session interaction.
#[derive(Debug, Default)]
pub struct Sessions;

impl App for Sessions {
    type Event = SessionEvent;
    type Model = Model;
    type ViewModel = SessionViewModel;
    type Effect = Effect;

    fn update(&self, event: SessionEvent, model: &mut Model) -> SessionCommand {
        match event {
            SessionEvent::Connect(context) => {
                if let Err(error) = validation::context(&context) {
                    return action_error(model, error);
                }
                if model.context() == Some(&context) {
                    return Command::done();
                }
                model.requests.clear();
                model.state = State {
                    context: Some(context),
                    ..State::default()
                };
                refresh(model)
            }
            SessionEvent::Refresh => refresh(model),
            SessionEvent::Select(id) => {
                if id.as_ref().is_some_and(|id| !model.state.visible(id)) {
                    return action_error(
                        model,
                        validation::error(
                            SessionErrorCode::InvalidSelection,
                            "Session is not owned or actively shared with this contributor",
                        ),
                    );
                }
                model.state.selected = id;
                model.state.action_error = None;
                render::render()
            }
            SessionEvent::Create { id, draft } => {
                mutations::start(model, id, SessionMutation::Create(draft))
            }
            SessionEvent::Submit { id, text } => {
                let Some(session_id) = model.state.selected.clone() else {
                    return no_selection(model);
                };
                mutations::start(model, id, SessionMutation::Submit { session_id, text })
            }
            SessionEvent::Invite {
                id,
                grant_id,
                grantee,
                permissions,
                expires_at_ms,
            } => {
                let Some(session_id) = model.state.selected.clone() else {
                    return no_selection(model);
                };
                mutations::start(
                    model,
                    id,
                    SessionMutation::Invite {
                        session_id,
                        grant_id,
                        grantee,
                        permissions,
                        expires_at_ms,
                    },
                )
            }
            SessionEvent::Revoke { id, grant_id } => mutations::revoke(model, id, grant_id),
            SessionEvent::Retry(id) => mutations::retry(model, &id),
            SessionEvent::Recover(id) => mutations::recover(model, &id),
            SessionEvent::RefreshInput(input) => {
                if !model.state.ready()
                    || !model.state.visible(&input.session_id)
                    || !model
                        .state
                        .prompts
                        .iter()
                        .any(|prompt| prompt.input == input)
                {
                    return action_error(
                        model,
                        validation::error(
                            SessionErrorCode::InvalidSelection,
                            "Input is not in the current authorized session view",
                        ),
                    );
                }
                request(model, SessionAction::InputStatus(input))
            }
            SessionEvent::Tick(now_ms) => {
                if now_ms <= model.state.now_ms {
                    return Command::done();
                }
                model.state.now_ms = now_ms;
                model.state.prune();
                retire_inaccessible(model);
                render::render()
            }
            SessionEvent::Disconnect => {
                model.requests.clear();
                model.state = State::default();
                render::render()
            }
            SessionEvent::Completed { token, result } => complete(model, token, result),
        }
    }

    fn view(&self, model: &Model) -> SessionViewModel {
        model.state.view()
    }
}

pub(super) fn action_error(model: &mut Model, error: SessionError) -> SessionCommand {
    model.state.action_error = Some(error);
    render::render()
}

pub(super) fn no_selection(model: &mut Model) -> SessionCommand {
    action_error(
        model,
        validation::error(
            SessionErrorCode::InvalidSelection,
            "Select a session before performing this action",
        ),
    )
}

pub(super) fn refresh(model: &mut Model) -> SessionCommand {
    if model.context().is_none() {
        return Command::done();
    }
    model.requests.retain(|operation| {
        !matches!(
            operation.action,
            SessionAction::Snapshot | SessionAction::Watch { .. }
        )
    });
    model.state.load = SessionLoadState::Loading;
    model.state.updates = SessionLoadState::Idle;
    request(model, SessionAction::Snapshot)
}

pub(super) fn watch(model: &mut Model) -> SessionCommand {
    let Some(snapshot) = &model.state.snapshot else {
        return Command::done();
    };
    let after = snapshot.cursor.clone();
    model.state.updates = SessionLoadState::Loading;
    request(model, SessionAction::Watch { after })
}

pub(super) fn request(model: &mut Model, action: SessionAction) -> SessionCommand {
    let Some(context) = model.context().cloned() else {
        return Command::done();
    };
    let operation = SessionOperation { context, action };
    if model.requests.values().any(|pending| pending == &operation) {
        return Command::done();
    }
    let token = match model.requests.register(operation.clone(), false) {
        Ok(token) => token,
        Err(_error) => {
            return fail(
                model,
                &operation,
                validation::invalid("Session request identity exhausted"),
            );
        }
    };
    Command::request_from_shell(operation)
        .then_send(move |result| SessionEvent::Completed { token, result })
        .and(render::render())
}

fn complete(model: &mut Model, token: u64, result: SessionOutput) -> SessionCommand {
    let Some((operation, _window)) = model.requests.take(token) else {
        return Command::done();
    };
    if model.context() != Some(&operation.context) {
        return Command::done();
    }
    let result = result.and_then(|result| match &operation.action {
        SessionAction::Snapshot | SessionAction::Watch { .. } => {
            recovery::accept(model, &operation, result)
        }
        SessionAction::Mutate { .. }
        | SessionAction::RequestStatus(_)
        | SessionAction::InputStatus(_) => mutations::complete(model, &operation, result),
    });
    result.unwrap_or_else(|error| fail(model, &operation, error))
}

pub(super) fn retire_inaccessible(model: &mut Model) {
    let state = &model.state;
    let retired: Vec<_> = model
        .requests
        .values()
        .filter_map(|operation| match &operation.action {
            SessionAction::Mutate { request, mutation }
                if mutation.session_id().is_some_and(|id| !state.visible(id)) =>
            {
                Some(request.key())
            }
            SessionAction::Snapshot
            | SessionAction::Watch { .. }
            | SessionAction::Mutate { .. }
            | SessionAction::InputStatus(_)
            | SessionAction::RequestStatus(_) => None,
        })
        .collect();
    model.requests.retain(|operation| match &operation.action {
        SessionAction::Mutate { mutation, .. } => {
            mutation.session_id().is_none_or(|id| state.visible(id))
        }
        SessionAction::InputStatus(input) => state.visible(&input.session_id),
        SessionAction::Snapshot | SessionAction::Watch { .. } | SessionAction::RequestStatus(_) => {
            true
        }
    });
    for mutation in &mut model.state.mutations {
        if retired.contains(&mutation.request.key())
            && mutation.state == super::SessionMutationState::Pending
        {
            mutation.state = super::SessionMutationState::Unknown;
        }
    }
}

fn fail(model: &mut Model, operation: &SessionOperation, error: SessionError) -> SessionCommand {
    match &operation.action {
        SessionAction::Snapshot | SessionAction::Watch { .. } => {
            if matches!(
                error.code,
                SessionErrorCode::Unauthenticated
                    | SessionErrorCode::Forbidden
                    | SessionErrorCode::NotFound
            ) {
                model.requests.clear();
                model.state.snapshot = None;
                model.state.selected = None;
                model.state.prompts.clear();
                model.state.mutations.clear();
            }
            match operation.action {
                SessionAction::Snapshot => model.state.load = SessionLoadState::Failed(error),
                SessionAction::Watch { .. } => {
                    model.state.updates = SessionLoadState::Failed(error);
                }
                SessionAction::Mutate { .. }
                | SessionAction::RequestStatus(_)
                | SessionAction::InputStatus(_) => {}
            }
        }
        SessionAction::Mutate { request, .. } => {
            if let Some(mutation) = model
                .state
                .mutations
                .iter_mut()
                .find(|mutation| mutation.request == *request)
                && !matches!(
                    mutation.state,
                    super::SessionMutationState::Acknowledged(_)
                        | super::SessionMutationState::Created(_)
                        | super::SessionMutationState::RuntimeReported
                )
            {
                mutation.state = super::SessionMutationState::Failed(error);
            }
        }
        SessionAction::RequestStatus(_) | SessionAction::InputStatus(_) => {
            model.state.action_error = Some(error);
        }
    }
    render::render()
}
