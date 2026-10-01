//! Root composition of domain models, events and views.

use crux_core::{App, Command, render};
use serde::{Deserialize, Serialize};

use crate::{bootstrap, effects::Effect, history, workspace};

/// State owned by one client, partitioned by domain reducer.
#[derive(Debug, Default)]
pub struct Model {
    initialized: bool,
    bootstrap: bootstrap::Model,
    history: history::Model,
    workspace: workspace::Model,
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
    /// Shared history interaction and exact evidence state.
    pub history: history::ViewModel,
    /// Shared workspace/repository navigation, members and presence.
    pub workspace: workspace::ViewModel,
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
                if model.workspace.owns_history()
                    && (matches!(&event, history::Event::Connect(chain) if model.workspace.chain() != Some(chain.as_str()))
                        || matches!(event, history::Event::Disconnect))
                {
                    return Command::done();
                }
                return history::History
                    .update(event, &mut model.history)
                    .map_event(Event::History);
            }
            Event::Workspace(event) => {
                let before = (
                    model.workspace.chain().map(str::to_owned),
                    model.workspace.owns_history(),
                );
                let command = workspace::Workspace
                    .update(event, &mut model.workspace)
                    .map_event(Event::Workspace);
                let after = (
                    model.workspace.chain().map(str::to_owned),
                    model.workspace.owns_history(),
                );
                if before == after {
                    return command;
                }
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
        }
    }
}
