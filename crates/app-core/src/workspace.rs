//! Workspace navigation, membership and presence. Owner: f22/workspace-state.
//!
//! Hosts discover authorized workspaces and resolve [`WorkspaceOperation`] through
//! standalone or managed adapters. Each workspace has one immutable logical chain;
//! repository, host and provider identities can occur in several workspace scopes.
//! Only the selected chain reference is passed to the history engine.

mod adapter;
mod model;
mod reducer;
mod types;
mod validation;

pub use model::Model;
pub use reducer::{Workspace, WorkspaceEvent as Event};
pub use types::WorkspaceViewModel as ViewModel;
pub use types::*;

#[cfg(test)]
mod tests;
