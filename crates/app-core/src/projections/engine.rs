//! Application projection adapter over shared native reads.

use super::{ProjectionOutput, ProjectionQuery};
use crate::module::EffectError;
use editchain_core::OpId;
use editchain_engine::queries::{ChainQueries, HistoryEntry};
use idle_protocol::v1::projections::{ProjectionInput, ProjectionReference};

pub use idle_history_native::projections::{
    ProjectionRead, ProjectionRecord, UnavailableMapper, source_reference,
};

/// Application-owned mapping of recorded controller state into derived inputs.
pub trait ProjectionMapper {
    /// Return each derived input without changing exact source identities.
    ///
    /// # Errors
    /// Returns an application failure when a replacement cannot be produced.
    fn inputs(
        &self,
        queries: &ChainQueries,
        read: &ProjectionRead,
    ) -> Result<Vec<ProjectionInput>, EffectError>;
}

impl ProjectionMapper for UnavailableMapper {
    fn inputs(
        &self,
        queries: &ChainQueries,
        read: &ProjectionRead,
    ) -> Result<Vec<ProjectionInput>, EffectError> {
        idle_history_native::projections::ProjectionMapper::inputs(self, queries, read)
            .map_err(Into::into)
    }
}

struct Mapper<'a, T>(&'a T);
impl<T: ProjectionMapper> idle_history_native::projections::ProjectionMapper for Mapper<'_, T> {
    fn inputs(
        &self,
        queries: &ChainQueries,
        read: &ProjectionRead,
    ) -> Result<Vec<ProjectionInput>, idle_history::query::Error> {
        self.0.inputs(queries, read).map_err(Into::into)
    }
}

/// Execute a projection effect with the caller's controller mapping.
///
/// # Errors
/// Preserves context, candidate bounds, content and mapping failures.
pub fn execute(
    queries: &mut ChainQueries,
    chain: &str,
    query: &ProjectionQuery,
    mapper: &impl ProjectionMapper,
) -> ProjectionOutput {
    idle_history_native::projections::execute(queries, chain, &query.into(), &Mapper(mapper))
        .map_err(Into::into)
}

/// Resolve an observation while preserving full recorded identities.
///
/// # Errors
/// Returns record-lookup failures from the shared reader.
pub fn observation_reference(
    queries: &ChainQueries,
    id: OpId,
) -> Result<ProjectionReference, EffectError> {
    idle_history_native::projections::observation_reference(queries, id).map_err(Into::into)
}

/// Return recorded history endpoints without combining item and observation IDs.
///
/// # Errors
/// Returns record-lookup failures from the shared reader.
pub fn related_references(
    queries: &ChainQueries,
    read: &ProjectionRead,
    entry: &HistoryEntry,
) -> Result<Vec<ProjectionReference>, EffectError> {
    idle_history_native::projections::related_references(queries, read, entry).map_err(Into::into)
}
