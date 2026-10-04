//! Context-bound replacement reads shared by standalone and managed adapters.

use crux_core::capability::Operation;
use serde::{Deserialize, Serialize};

use super::ProjectionSnapshot;
use crate::{module::EffectError, subscriptions::Context};

/// Read all five destinations for one authorized audience and chain.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ProjectionQuery {
    /// Explicit provider, workspace, authenticated contributor and logical chain.
    pub context: Context,
    /// Maximum engine candidates to read, from 1 to 1000; not a result total.
    pub limit: u32,
    /// Revalidate upstream sources instead of reusing a recent read.
    #[serde(default)]
    pub refresh_sources: bool,
}

/// Typed Rust host output, validated before any rows are replaced.
pub type ProjectionOutput = Result<ProjectionSnapshot, EffectError>;

/// Generated-shell response with the binary layout of [`ProjectionOutput`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ProjectionResponse {
    /// Complete atomic input envelope, with explicit partial/unavailable views.
    Ok(ProjectionSnapshot),
    /// Presentable host failure; retained rows are marked stale.
    Err(EffectError),
}

impl Operation for ProjectionQuery {
    type Output = ProjectionOutput;
}
