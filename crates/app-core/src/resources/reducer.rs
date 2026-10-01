//! Resource reducer: pure state transitions with host-executed effects.

use crux_core::{App, Command, render};
use serde::{Deserialize, Serialize};

use super::{
    Model, ResourceContext, ResourceError, ResourceLoadState, ResourceMutation, ResourceOperation,
    ResourceOperationKind, ResourceOutput, ResourceRequest, ResourceResult, ViewModel, mutations,
    validation,
};
use crate::Effect;

pub(super) type ResourceCommand = Command<Effect, ResourceEvent>;

/// Client resource intents. Effect results cannot be forged by serialized events.
#[derive(Clone, Debug, Deserialize, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ResourceEvent {
    /// Bind an authenticated workspace and load its resources.
    Connect(ResourceContext),
    /// Refresh discovery and reconcile outstanding actions by original identity.
    Refresh,
    /// Select a visible host for navigation, without connecting to it.
    SelectHost(Option<String>),
    /// Select a visible provider for navigation, without adopting a model.
    SelectProvider(Option<String>),
    /// Attempt one currently allowed runtime operation.
    Execute {
        /// Host-persisted retry identity and original deadline.
        request: ResourceRequest,
        /// Exact resource intent; no action is performed inside the reducer.
        mutation: ResourceMutation,
    },
    /// Retry only after explicit adapter advice, retaining original payload/identity.
    Retry(String),
    /// Reconcile the original request without executing it again.
    CheckStatus(String),
    /// Update the provider-aligned Unix clock; backward ticks are ignored.
    AdvanceClock(u64),
    /// Retire continuations after losing delivery continuity.
    Suspend,
    /// Refresh the same context before recovering pending operations.
    Reconnect,
    /// Retire all work and clear this client's resource scope.
    Disconnect,
    /// Restore a host-persisted action as uncertain and query its original status.
    /// An elapsed first-receipt deadline does not prevent restoration or lookup.
    Restore {
        /// Original binding; must exactly match the connected resource context.
        context: ResourceContext,
        /// Persisted identity and unchanged first-receipt deadline.
        request: ResourceRequest,
        /// Original immutable intent, never executed by this event.
        mutation: ResourceMutation,
    },
    /// Internal host continuation, excluded from serialized client actions.
    #[serde(skip)]
    #[facet(skip)]
    Completed {
        /// Non-reusable local continuation identity.
        token: u64,
        /// Host result for the corresponding resource operation.
        result: ResourceOutput,
    },
}

/// Shared resource behavior; no serving, authorization or controller execution.
#[derive(Debug, Default)]
pub struct Resources;

impl App for Resources {
    type Event = ResourceEvent;
    type Model = Model;
    type ViewModel = ViewModel;
    type Effect = Effect;

    fn update(&self, event: ResourceEvent, model: &mut Model) -> ResourceCommand {
        match event {
            ResourceEvent::Connect(context) => {
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
            ResourceEvent::Refresh => refresh(model),
            ResourceEvent::SelectHost(id) => {
                if id.as_ref().is_some_and(|id| {
                    !model
                        .snapshot
                        .as_ref()
                        .is_some_and(|snapshot| snapshot.hosts.iter().any(|host| &host.id == id))
                }) {
                    return action_error(
                        model,
                        validation::selection("Host is not visible in this workspace"),
                    );
                }
                model.selected_host = id;
                model.action_error = None;
                render::render()
            }
            ResourceEvent::SelectProvider(id) => {
                if id.as_ref().is_some_and(|id| {
                    !model.snapshot.as_ref().is_some_and(|snapshot| {
                        snapshot.providers.iter().any(|provider| &provider.id == id)
                    })
                }) {
                    return action_error(
                        model,
                        validation::selection("Provider is not visible in this workspace"),
                    );
                }
                model.selected_provider = id;
                model.action_error = None;
                render::render()
            }
            ResourceEvent::Execute { request, mutation } => {
                mutations::execute(model, request, mutation)
            }
            ResourceEvent::Restore {
                context,
                request,
                mutation,
            } => mutations::restore(model, &context, request, mutation),
            ResourceEvent::Retry(id) => {
                mutations::recover(model, &id, super::ResourceRecoveryAction::Retry)
            }
            ResourceEvent::CheckStatus(id) => {
                mutations::recover(model, &id, super::ResourceRecoveryAction::CheckStatus)
            }
            ResourceEvent::AdvanceClock(now_ms) => {
                model.now_ms = model.now_ms.max(now_ms);
                render::render()
            }
            ResourceEvent::Suspend => {
                if model.context.is_none() {
                    return Command::done();
                }
                model.suspend();
                render::render()
            }
            ResourceEvent::Reconnect => {
                if model.context.is_none() {
                    return Command::done();
                }
                model.suspend();
                model.load = ResourceLoadState::Idle;
                refresh(model)
            }
            ResourceEvent::Disconnect => {
                model.reset();
                render::render()
            }
            ResourceEvent::Completed { token, result } => complete(model, token, result),
        }
    }

    fn view(&self, model: &Model) -> ViewModel {
        model.view()
    }
}

pub(super) fn action_error(model: &mut Model, error: ResourceError) -> ResourceCommand {
    model.action_error = Some(error);
    render::render()
}

pub(super) fn dispatch(model: &mut Model, kind: ResourceOperationKind) -> ResourceCommand {
    let Some(context) = model.context.clone() else {
        return Command::done();
    };
    let operation = ResourceOperation { context, kind };
    let Ok(token) = model.requests.register(operation.clone(), false) else {
        let error = validation::invalid("Resource continuation identities exhausted");
        if operation.kind == ResourceOperationKind::Snapshot {
            model.load = ResourceLoadState::Failed(error.clone());
        } else {
            mutations::failed(model, &operation.kind, error.clone());
        }
        return action_error(model, error);
    };
    Command::request_from_shell(operation)
        .then_send(move |result| ResourceEvent::Completed { token, result })
        .and(render::render())
}

pub(super) fn refresh(model: &mut Model) -> ResourceCommand {
    if model.context.is_none() || model.load == ResourceLoadState::Suspended {
        return Command::done();
    }
    mutations::invalidate(model);
    if model
        .requests
        .values()
        .any(|operation| operation.kind == ResourceOperationKind::Snapshot)
    {
        model.refresh_again = true;
        return Command::done();
    }
    start_snapshot(model)
}

fn start_snapshot(model: &mut Model) -> ResourceCommand {
    model.load = ResourceLoadState::Loading;
    model.action_error = None;
    dispatch(model, ResourceOperationKind::Snapshot)
}

fn complete(model: &mut Model, token: u64, result: ResourceOutput) -> ResourceCommand {
    let Some((operation, _window)) = model.requests.take(token) else {
        return Command::done();
    };
    if model.context() != Some(&operation.context) {
        return Command::done();
    }
    if operation.kind != ResourceOperationKind::Snapshot {
        return mutations::complete(model, &operation.kind, result);
    }
    let result = result.and_then(|result| match result {
        ResourceResult::Snapshot(snapshot) => {
            validation::snapshot(&snapshot, &operation.context, model.snapshot.as_ref())?;
            for grant in &snapshot.grants {
                if let Some(previous) = model.known_grants.get(&grant.id) {
                    validation::grant_update(grant, previous)?;
                }
            }
            Ok(snapshot)
        }
        ResourceResult::Progress(_) | ResourceResult::Unknown => Err(validation::invalid(
            "Expected a resource discovery replacement",
        )),
    });
    match result {
        Ok(snapshot) => {
            model.now_ms = model.now_ms.max(snapshot.now_ms);
            for grant in &snapshot.grants {
                drop(model.known_grants.insert(grant.id.clone(), grant.clone()));
            }
            if model
                .selected_host
                .as_ref()
                .is_some_and(|id| !snapshot.hosts.iter().any(|host| &host.id == id))
            {
                model.selected_host = None;
            }
            if model
                .selected_provider
                .as_ref()
                .is_some_and(|id| !snapshot.providers.iter().any(|provider| &provider.id == id))
            {
                model.selected_provider = None;
            }
            model.snapshot = Some(*snapshot);
            model.load = ResourceLoadState::Ready;
        }
        Err(error) => {
            if matches!(
                error.code,
                super::ResourceErrorCode::Forbidden | super::ResourceErrorCode::Unauthenticated
            ) {
                model.requests.clear();
                model.snapshot = None;
                model.selected_host = None;
                model.selected_provider = None;
                model.mutations.clear();
                model.status_dirty.clear();
                model.refresh_again = false;
            }
            model.load = ResourceLoadState::Failed(error);
        }
    }
    let command = mutations::reconcile(model);
    if std::mem::take(&mut model.refresh_again) {
        return command.and(start_snapshot(model));
    }
    command.and(render::render())
}
