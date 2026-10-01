//! Root composition of domain models, events and views.

use crux_core::{App, Command, render};
use serde::{Deserialize, Serialize};

use crate::{
    bootstrap, effects::Effect, history, projections, resources, sessions, subscriptions, workspace,
};

/// State owned by one client, partitioned by domain reducer.
#[derive(Debug, Default)]
pub struct Model {
    initialized: bool,
    bootstrap: bootstrap::Model,
    history: history::Model,
    workspace: workspace::Model,
    subscriptions: subscriptions::Model,
    sessions: sessions::Model,
    projections: projections::Model,
    resources: resources::Model,
}

/// Client actions and domain events accepted by the application.
#[derive(Clone, Debug, Deserialize, Serialize, facet::Facet)]
#[repr(u8)]
pub enum Event {
    /// Initialize the client and load host information.
    Start,
    /// Route an action to the bootstrap domain.
    Bootstrap(bootstrap::Event),
    /// Route a semantic history action.
    History(history::Event),
    /// Route workspace selection, membership and presence actions.
    Workspace(workspace::Event),
    /// Route shared joins, reconnects and subscription lifetimes.
    Subscriptions(subscriptions::Event),
    /// Route owned/invited sessions, sharing and attributed prompts.
    Sessions(sessions::Event),
    /// Route projection selection, filtering, reads and history drill-down.
    Projections(projections::Event),
    /// Route compute/provider discovery, model actions and controller state.
    Resources(resources::Event),
}

/// The typed presentation state shared by all client surfaces.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct ViewModel {
    /// Whether the client has processed its start event (not host readiness).
    pub initialized: bool,
    /// Loading, error or host information from the bootstrap reducer.
    pub bootstrap: bootstrap::ViewModel,
    /// Shared history interaction, stored records and field content.
    pub history: history::ViewModel,
    /// Shared workspace/repository navigation, members and presence.
    pub workspace: workspace::ViewModel,
    /// Shared connection status and reconciliation readiness.
    pub subscriptions: subscriptions::SubscriptionViewModel,
    /// Session selection, pending mutations and runtime-confirmed input facts.
    pub sessions: sessions::ViewModel,
    /// Activity, task, error, triage and need-input views with source references.
    pub projections: projections::ViewModel,
    /// Resource availability, allowed model actions, progress and controller status.
    pub resources: resources::ViewModel,
}

/// Root reducer composing the shared application's domain modules.
#[derive(Debug, Default)]
pub struct IdleApp;

impl App for IdleApp {
    type Event = Event;
    type Model = Model;
    type ViewModel = ViewModel;
    type Effect = Effect;

    fn update(&self, event: Event, model: &mut Model) -> Command<Effect, Event> {
        let event = match event {
            Event::Start => {
                if model.initialized {
                    return Command::done();
                }
                model.initialized = true;
                // Starting after an explicit module load must still render this change.
                return bootstrap::Bootstrap
                    .update(bootstrap::Event::Load, &mut model.bootstrap)
                    .map_event(Event::Bootstrap)
                    .and(render::render());
            }
            Event::Bootstrap(event) => event,
            Event::History(event) => {
                // Workspace navigation owns the binding while connected. Direct
                // chain selection remains available to independent history clients.
                if model.subscriptions.context().is_some()
                    && matches!(
                        event,
                        history::Event::Connect(_) | history::Event::Disconnect
                    )
                {
                    return Command::done();
                }
                if model.workspace.owns_history()
                    && (matches!(&event, history::Event::Connect(chain) if model.workspace.chain() != Some(chain.as_str()))
                        || matches!(event, history::Event::Disconnect))
                {
                    return Command::done();
                }
                let command = history::History
                    .update(event, &mut model.history)
                    .map_event(Event::History);
                model
                    .subscriptions
                    .history_state(model.history.reconciliation());
                return command;
            }
            Event::Subscriptions(event) => return update_subscription(event, model),
            Event::Projections(event) => return update_projection(event, model),
            Event::Resources(event) => return update_resources(event, model),
            Event::Sessions(event) => {
                if let sessions::Event::Connect(context) = &event
                    && ((model.workspace.owns_history()
                        && (model.workspace.chain() != Some(context.chain.as_str())
                            || model.workspace.workspace_id()
                                != Some(context.workspace_id.as_str())
                            || model.workspace.coordination_mode() != Some(context.mode)))
                        || model.subscriptions.context().is_some_and(|active| {
                            active.workspace != context.workspace_id
                                || active.chain != context.chain
                                || active.contributor != context.contributor_id
                                || active.provider != context.provider
                        }))
                {
                    return Command::done();
                }
                return sessions::Sessions
                    .update(event, &mut model.sessions)
                    .map_event(Event::Sessions);
            }
            Event::Workspace(event) => {
                let before = (
                    model.workspace.chain().map(str::to_owned),
                    model.workspace.owns_history(),
                    model.workspace.workspace_id().map(str::to_owned),
                    model.workspace.coordination_mode(),
                );
                let command = workspace::Workspace
                    .update(event, &mut model.workspace)
                    .map_event(Event::Workspace);
                let after = (
                    model.workspace.chain().map(str::to_owned),
                    model.workspace.owns_history(),
                    model.workspace.workspace_id().map(str::to_owned),
                    model.workspace.coordination_mode(),
                );
                if before == after {
                    return command;
                }
                let command =
                    command.and(update_subscription(subscriptions::Event::Disconnect, model));
                let command = command.and(
                    sessions::Sessions
                        .update(sessions::Event::Disconnect, &mut model.sessions)
                        .map_event(Event::Sessions),
                );
                let command = command.and(update_projection(projections::Event::Disconnect, model));
                let command = command.and(update_resources(resources::Event::Disconnect, model));
                model.history.bind(None);
                let event = after
                    .0
                    .map_or(history::Event::Disconnect, history::Event::Connect);
                return command.and(
                    history::History
                        .update(event, &mut model.history)
                        .map_event(Event::History),
                );
            }
        };
        bootstrap::Bootstrap
            .update(event, &mut model.bootstrap)
            .map_event(Event::Bootstrap)
    }

    fn view(&self, model: &Model) -> ViewModel {
        ViewModel {
            initialized: model.initialized,
            bootstrap: bootstrap::Bootstrap.view(&model.bootstrap),
            history: history::History.view(&model.history),
            workspace: workspace::Workspace.view(&model.workspace),
            subscriptions: subscriptions::Subscriptions.view(&model.subscriptions),
            sessions: sessions::Sessions.view(&model.sessions),
            projections: projections::Projections.view(&model.projections),
            resources: resources::Resources.view(&model.resources),
        }
    }
}

fn update_subscription(event: subscriptions::Event, model: &mut Model) -> Command<Effect, Event> {
    if let subscriptions::Event::Connect(context) = &event
        && model.workspace.owns_history()
        && (model.workspace.chain() != Some(context.chain.as_str())
            || model.workspace.workspace_id() != Some(context.workspace.as_str()))
    {
        return Command::done();
    }
    let before = model.subscriptions.context().cloned();
    let command = subscriptions::Subscriptions
        .update(event, &mut model.subscriptions)
        .map_event(Event::Subscriptions);
    let after = model.subscriptions.context();
    if before.as_ref() != after {
        model
            .history
            .bind(after.map(|context| context.chain.clone()));
    }
    let command = if before.as_ref() != after
        && model
            .projections
            .context()
            .is_some_and(|context| after != Some(context))
    {
        command.and(
            projections::Projections
                .update(projections::Event::Disconnect, &mut model.projections)
                .map_event(Event::Projections),
        )
    } else {
        command
    };
    let command = if after.is_some_and(|active| {
        model.sessions.context().is_some_and(|context| {
            context.workspace_id != active.workspace
                || context.chain != active.chain
                || context.contributor_id != active.contributor
                || context.provider != active.provider
        })
    }) {
        command.and(
            sessions::Sessions
                .update(sessions::Event::Disconnect, &mut model.sessions)
                .map_event(Event::Sessions),
        )
    } else {
        command
    };
    let command = if before.as_ref() != after
        && model.resources.context().is_some_and(|context| {
            !after.is_some_and(|active| resource_subscription_matches(context, active))
        }) {
        command.and(
            resources::Resources
                .update(resources::Event::Disconnect, &mut model.resources)
                .map_event(Event::Resources),
        )
    } else {
        command
    };
    if let Some(event) = model.subscriptions.take_history_event() {
        let projection_event = match &event {
            history::Event::Refresh => Some(projections::Event::Refresh),
            history::Event::Reconnect => Some(projections::Event::Reconnect),
            history::Event::Suspend => Some(projections::Event::Suspend),
            history::Event::Disconnect => Some(projections::Event::Disconnect),
            history::Event::Connect(_)
            | history::Event::SetFilter(_)
            | history::Event::LoadMore
            | history::Event::Search(_)
            | history::Event::SearchMore
            | history::Event::NavigateMatch(_)
            | history::Event::Select(_)
            | history::Event::ClearSelection
            | history::Event::ToggleDisclosure(_)
            | history::Event::LoadItem(_)
            | history::Event::LoadOperationDetails { .. }
            | history::Event::Open { .. }
            | history::Event::Completed { .. } => None,
        };
        let resource_event = match &event {
            history::Event::Refresh => Some(resources::Event::Refresh),
            history::Event::Reconnect => Some(resources::Event::Reconnect),
            history::Event::Suspend => Some(resources::Event::Suspend),
            history::Event::Disconnect => Some(resources::Event::Disconnect),
            history::Event::Connect(_)
            | history::Event::SetFilter(_)
            | history::Event::LoadMore
            | history::Event::Search(_)
            | history::Event::SearchMore
            | history::Event::NavigateMatch(_)
            | history::Event::Select(_)
            | history::Event::ClearSelection
            | history::Event::ToggleDisclosure(_)
            | history::Event::LoadItem(_)
            | history::Event::LoadOperationDetails { .. }
            | history::Event::Open { .. }
            | history::Event::Completed { .. } => None,
        };
        let command = if let Some(event) = resource_event {
            command.and(update_resources(event, model))
        } else {
            command
        };
        let command = if let Some(event) = projection_event {
            command.and(update_projection(event, model))
        } else {
            command
        };
        let history = history::History
            .update(event, &mut model.history)
            .map_event(Event::History);
        command.and(history)
    } else {
        command
    }
}

fn update_projection(event: projections::Event, model: &mut Model) -> Command<Effect, Event> {
    if let projections::Event::Connect(context) = &event
        && ((model.workspace.owns_history()
            && (model.workspace.chain() != Some(context.chain.as_str())
                || model.workspace.workspace_id() != Some(context.workspace.as_str())))
            || model
                .subscriptions
                .context()
                .is_some_and(|active| active != context))
    {
        return Command::done();
    }
    if model.subscriptions.context().is_some() && !model.subscriptions.has_connection() {
        if let projections::Event::Connect(context) = &event {
            model.projections.wait_for_connection(context.clone());
            return render::render();
        }
        if matches!(event, projections::Event::Reconnect) {
            return projections::Projections
                .update(projections::Event::Suspend, &mut model.projections)
                .map_event(Event::Projections);
        }
    }
    let command = projections::Projections
        .update(event, &mut model.projections)
        .map_event(Event::Projections);
    let Some(event) = model.projections.take_history_event() else {
        return command;
    };
    let Some(context) = model.projections.context() else {
        return command;
    };
    let command = command.and(
        history::History
            .update(
                history::Event::Connect(context.chain.clone()),
                &mut model.history,
            )
            .map_event(Event::History),
    );
    command.and(
        history::History
            .update(event, &mut model.history)
            .map_event(Event::History),
    )
}

fn resource_subscription_matches(
    context: &resources::ResourceContext,
    active: &subscriptions::Context,
) -> bool {
    context.workspace_id == active.workspace
        && context.chain == active.chain
        && context.contributor_id == active.contributor
        && context.provider == active.provider
}

fn update_resources(event: resources::Event, model: &mut Model) -> Command<Effect, Event> {
    if let resources::Event::Connect(context) = &event
        && ((model.workspace.owns_history()
            && (model.workspace.chain() != Some(context.chain.as_str())
                || model.workspace.workspace_id() != Some(context.workspace_id.as_str())
                || model.workspace.coordination_mode() != Some(context.mode)))
            || model
                .subscriptions
                .context()
                .is_some_and(|active| !resource_subscription_matches(context, active)))
    {
        return Command::done();
    }
    if model.subscriptions.context().is_some() && !model.subscriptions.has_connection() {
        if let resources::Event::Connect(context) = &event {
            model.resources.wait_for_connection(context.clone());
            return render::render();
        }
        if matches!(event, resources::Event::Reconnect) {
            return resources::Resources
                .update(resources::Event::Suspend, &mut model.resources)
                .map_event(Event::Resources);
        }
    }
    resources::Resources
        .update(event, &mut model.resources)
        .map_event(Event::Resources)
}
