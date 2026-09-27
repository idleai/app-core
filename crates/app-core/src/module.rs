//! Domain modules use Crux's own reducer contract, re-exported as [`Module`].
//!
//! Each module owns its `Model`, `Event` and `ViewModel`. Its `update` method only
//! changes state and returns a [`Command`]; platform work belongs to host adapters.
//! The root app routes events to the module's model and lifts the returned command
//! with `map_event` (and `map_effect` for module-local effects). Commands retain
//! their typed continuations during this mapping. See [`crate::bootstrap`].
//!
//! Add domain state and view fields in the root composition, add an event variant,
//! and register new operations in [`crate::effects`]. A completion event should
//! use `#[serde(skip)]` and `#[facet(skip)]` so serialized client actions cannot
//! forge effect results and generated host types expose only client actions.
//! Subscriptions and context-specific reconciliation belong to their domain;
//! request IDs at the shell boundary only correlate in-flight effect continuations.

use serde::{Deserialize, Serialize};

/// The shared reducer interface, identical for a root app and a domain module.
pub use crux_core::App as Module;
/// Composable, testable descriptions of host work and follow-up events.
pub use crux_core::Command;

/// A host-reported operation failure suitable for presentation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct EffectError {
    /// A host-supplied explanation, without credentials or private diagnostics.
    pub message: String,
}

/// Common state for a domain's asynchronously loaded value.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum LoadState<T> {
    /// No load has been requested.
    #[default]
    Idle,
    /// An operation is awaiting its host result.
    Loading,
    /// The host supplied the requested value.
    Ready(T),
    /// The host reported a failure; the domain can offer a retry.
    Failed(EffectError),
}
