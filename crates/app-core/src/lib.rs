//! Shared application behavior. Hosts execute effects; renderers consume views.
//! The bootstrap is the only implemented behavior in the f1 scaffold.

mod bootstrap;
pub mod configuration;
pub mod history;
pub mod projections;
pub mod resources;
pub mod sessions;
pub mod subscriptions;
pub mod workspace;

pub use bootstrap::{Effect, Event, IdleApp, Model, ViewModel};

/// One application's independent Crux state.
pub type Core = crux_core::Core<IdleApp>;
