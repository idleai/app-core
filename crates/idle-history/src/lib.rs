//! Portable semantic contracts shared by app-core and transitional viewer clients.
//! Crux owns history state in app-core; this small package keeps the old protocol
//! and renderer independent of the Crux runtime during their staged migration.

pub mod connection;
pub mod legacy;
mod presentation;
pub mod reconciliation;
pub mod requests;
mod selection;

pub use presentation::{ContentText, MAX_ROW_TEXT_BYTES, MAX_TOOL_LABEL_BYTES, RowContent};
pub use selection::Selection;
