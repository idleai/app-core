//! Workspace resources, runtime model actions and observed controller status.
//!
//! Health, discovery and permission are separate. Hosts authorize every effect;
//! directory publications and coordination receipts never establish runtime success.

mod adapter;
mod controller;
mod model;
mod mutations;
mod operations;
mod reducer;
mod types;
mod validation;
mod views;

#[cfg(any(test, feature = "resource-fixtures"))]
pub mod scripted;

pub use adapter::ResourceAdapterContext;
pub use controller::*;
pub use model::Model;
pub use operations::*;
pub use reducer::{ResourceEvent as Event, Resources};
pub use types::*;
pub use views::{ResourceViewModel as ViewModel, *};

#[cfg(test)]
mod tests;
