//! Shared application behavior. Hosts execute effects; renderers consume views.
//! See [`module`] for reducer composition and [`Shell`] for the JSON host boundary.

mod app;
pub mod bootstrap;
pub mod configuration;
pub mod effects;
pub mod history;
pub mod module;
pub mod projections;
pub mod resources;
pub mod sessions;
pub mod shell;
pub mod subscriptions;
pub mod workspace;

pub use app::{Event, IdleApp, Model, ViewModel};
pub use effects::Effect;
pub use shell::{Shell, ShellError};

/// One application's independent Crux state.
pub type Core = crux_core::Core<IdleApp>;

#[cfg(test)]
mod tests;
