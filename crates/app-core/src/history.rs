//! Shared semantic history state. Owner: f23/history-state.
//!
//! Item keys identify logical objects; observation and record references identify
//! stored operation bytes. Hosts execute [`Query`] through engine adapters and return results
//! through Crux continuations. No row coordinates, scrolling, DOM or graph layout
//! are part of this module. Query cursors describe bounded scans, not subscriptions.

mod cache;
mod model;
mod reconciliation;
mod reducer;
mod types;
mod views;

#[cfg(not(target_arch = "wasm32"))]
pub mod engine;

pub use idle_history::{
    ContentText, MAX_ROW_TEXT_BYTES, MAX_TOOL_LABEL_BYTES, RowContent, Selection,
};
pub use model::Model;
pub use reconciliation::{ItemScan, ItemSnapshot, Reconcile, Reconciled};
pub use reducer::{History, HistoryEvent as Event};
pub use types::*;
pub use views::*;

#[cfg(test)]
mod tests;
