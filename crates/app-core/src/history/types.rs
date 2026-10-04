//! Crux history operations over shared query and result contracts.

use crate::module::EffectError;
use crux_core::capability::Operation;
use serde::{Deserialize, Serialize};

pub use idle_history::query::{
    ActivityKind, Comparison, ContentValue, FieldContent, Filter, HistoryPage, MatchRange,
    Observation, OpenTarget, OperationDetails, Page, QueryAction, QueryResult, RawRecord,
    RecordLookupStatus, RecordRef, SearchMatch, SearchPage,
};

/// Host operation scoped only to a caller-resolved logical chain.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct Query {
    /// Opaque chain binding resolved by the host independently of workspace UI.
    pub chain: String,
    /// Exact requested operation.
    pub action: QueryAction,
}

/// Result returned by typed Rust hosts.
pub type QueryOutput = Result<QueryResult, EffectError>;

/// Named wire result matching the binary layout of [`QueryOutput`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum QueryResponse {
    /// Successful host result.
    Ok(QueryResult),
    /// Presentable host failure.
    Err(EffectError),
}

impl Operation for Query {
    type Output = QueryOutput;
}

impl From<&Query> for idle_history::query::Query {
    fn from(value: &Query) -> Self {
        Self {
            chain: value.chain.clone(),
            action: value.action.clone(),
        }
    }
}
