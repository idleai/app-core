//! Versioned settings and agent-rule editors for either coordination provider.
//!
//! Only providers commit configuration. Evo interprets and enforces rules;
//! this module checks document shape and preserves drafts across asynchronous work.

mod adapter;
mod model;
mod operations;
mod reducer;
mod types;
mod validation;

pub use adapter::ConfigurationWrite;
pub use model::Model;
pub use operations::*;
pub use reducer::{Configuration, ConfigurationEvent as Event};
pub use types::*;

#[cfg(test)]
mod tests;
