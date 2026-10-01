//! Owned and invited sessions, explicit logical history bindings and attributed input.
//!
//! Hosts resolve [`SessionOperation`] using the published coordination contracts
//! and an authenticated runtime. Receipts never establish input order or execution.
//! Runtime creation and history identity allocation belong to the host adapter.

mod adapter;
mod identity;
mod input;
mod model;
mod mutations;
mod operations;
mod recovery;
mod reducer;
mod validation;
mod views;

#[cfg(any(test, feature = "session-fixtures"))]
pub mod scripted;

pub use adapter::SessionAdapterContext;
pub use identity::*;
pub use input::*;
pub use model::Model;
pub use operations::*;
pub use reducer::{SessionEvent as Event, Sessions};
pub use views::SessionViewModel as ViewModel;
pub use views::*;

#[cfg(test)]
mod tests;
