//! Root composition of domain models, events and views.

use crux_core::{App, Command, render};
use serde::{Deserialize, Serialize};

use crate::{bootstrap, effects::Effect, history, sessions, subscriptions, workspace};

/// State owned by one client, partitioned by domain reducer.
#[derive(Debug, Default)]
pub struct Model {
    initialized: bool,
    bootstrap: bootstrap::Model,
    history: history::Model,
    workspace: workspace::Model,
    subscriptions: subscriptions::Model,
    sessions: sessions::Model,
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
    if let Some(event) = model.subscriptions.take_history_event() {
        let history = history::History
            .update(event, &mut model.history)
            .map_event(Event::History);
        command.and(history)
    } else {
        command
    }
}
