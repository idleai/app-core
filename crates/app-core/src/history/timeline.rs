//! Bounded Activity interaction state. Routing is supplied by the native host.

mod model;
mod reducer;
mod response;

pub(super) use model::Model;
pub use model::{Search, Selection, Surface, SurfaceView, ViewModel};
pub use reducer::Event;
pub(super) use reducer::update;

pub use idle_history::timeline::{
    Address, Cursor, Geometry, Group, Match, Response, Row, Source, Transition, Window,
};
