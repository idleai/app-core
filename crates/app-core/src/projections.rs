//! Activity, task, error, triage and need-input state from shared provider inputs.
//!
//! Hosts map controller state through `idle_protocol::v1::projections`. The
//! reducer owns only selection, filters, replacement reads and freshness display.

pub mod adapter;
#[cfg(not(target_arch = "wasm32"))]
pub mod engine;
#[cfg(all(
    not(target_arch = "wasm32"),
    any(test, feature = "projection-fixtures")
))]
pub mod fixtures;
mod model;
mod operations;
mod reducer;
mod types;
mod views;

pub use model::Model;
pub use operations::{ProjectionOutput, ProjectionQuery, ProjectionResponse};
pub use reducer::{ProjectionEvent as Event, Projections};
pub use types::{
    FreshnessStatus, ProjectionAvailability, ProjectionFreshness, ProjectionGap, ProjectionInput,
    ProjectionKind, ProjectionReference, ProjectionRow, ProjectionSnapshot,
};
pub use views::{
    ProjectionFilter, ProjectionLoadState, ProjectionSelection, ProjectionView,
    ProjectionViewModel as ViewModel,
};

#[cfg(test)]
mod tests;
