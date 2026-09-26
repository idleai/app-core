//! Minimal Crux linkage; f21 owns the eventual runtime and platform bindings.

use crux_core::{
    App, Command, Request,
    render::{self, RenderOperation},
};
use serde::{Deserialize, Serialize};

/// State owned by one client during scaffold initialization.
#[derive(Debug, Default)]
pub struct Model {
    initialized: bool,
}

/// Client events understood by the scaffold.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Event {
    /// Initialize the client and request its first render.
    Start,
}

/// Requests dispatched from the application to its host.
#[derive(Debug)]
pub enum Effect {
    /// Refresh the host's rendered view.
    Render(Request<RenderOperation>),
}

impl crux_core::Effect for Effect {}

impl From<Request<RenderOperation>> for Effect {
    fn from(request: Request<RenderOperation>) -> Self {
        Self::Render(request)
    }
}

/// The typed state a host can present after processing events.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct ViewModel {
    /// Whether the initial client event has been handled.
    pub initialized: bool,
}

/// The minimal application used to verify Crux integration.
#[derive(Debug, Default)]
pub struct IdleApp;

impl App for IdleApp {
    type Event = Event;
    type Model = Model;
    type ViewModel = ViewModel;
    type Effect = Effect;

    fn update(&self, event: Event, model: &mut Model) -> Command<Effect, Event> {
        match event {
            Event::Start => model.initialized = true,
        }
        render::render()
    }

    fn view(&self, model: &Model) -> ViewModel {
        ViewModel {
            initialized: model.initialized,
        }
    }
}
