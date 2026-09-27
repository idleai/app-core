//! `BoltFFI` native bindings for the shared application core.

#[expect(
    clippy::used_underscore_items,
    reason = "BoltFFI's export macro calls its generated underscore-prefixed helpers"
)]
mod ffi;

pub use ffi::{AppCore, BindingError};
