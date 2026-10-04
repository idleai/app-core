//! Application result adapter over shared native history reads.

use super::{Query, QueryOutput};
use editchain_engine::queries::ChainQueries;

/// Execute a history effect through the shared native service implementation.
///
/// # Errors
/// Preserves binding, identity, content and storage failures from the shared reader.
pub fn execute(queries: &mut ChainQueries, chain: &str, query: &Query) -> QueryOutput {
    idle_history_native::query::execute(queries, chain, &query.into()).map_err(Into::into)
}
