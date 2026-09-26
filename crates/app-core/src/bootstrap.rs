//! A small domain reducer demonstrating host execution without platform I/O.

use crate::{
    effects::{Effect, HostInfo, HostInfoOperation, HostInfoResult},
    module::{Command, LoadState, Module},
};
use crux_core::render;
use serde::{Deserialize, Serialize};

/// Private interaction state belonging to the bootstrap module.
#[derive(Debug, Default)]
pub struct Model {
    state: LoadState<HostInfo>,
}

/// Actions routed to the bootstrap reducer.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum Event {
    /// Load the host's client information, or retry a failed load.
    Load,
    /// Internal continuation; shells must return results through the request ID.
    #[serde(skip)]
    Completed(HostInfoResult),
}

/// Bootstrap's presentation state, independent of any renderer.
pub type ViewModel = LoadState<HostInfo>;

/// Reducer for the first host handshake.
#[derive(Debug, Default)]
pub struct Bootstrap;

impl Module for Bootstrap {
    type Event = Event;
    type Model = Model;
    type ViewModel = ViewModel;
    type Effect = Effect;

    fn update(&self, event: Event, model: &mut Model) -> Command<Effect, Event> {
        match event {
            Event::Load => match model.state {
                LoadState::Loading | LoadState::Ready(_) => Command::done(),
                LoadState::Idle | LoadState::Failed(_) => {
                    model.state = LoadState::Loading;
                    Command::request_from_shell(HostInfoOperation)
                        .then_send(Event::Completed)
                        .and(render::render())
                }
            },
            Event::Completed(result) => {
                model.state = match result {
                    Ok(info) => LoadState::Ready(info),
                    Err(error) => LoadState::Failed(error),
                };
                render::render()
            }
        }
    }

    fn view(&self, model: &Model) -> ViewModel {
        model.state.clone()
    }
}
