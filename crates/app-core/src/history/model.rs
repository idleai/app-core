//! Reducer-owned interaction and cached query state.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    Filter, OperationDetailsState, Paging, QueryAction, RequestState, SearchView, Selected,
    cache::Cache,
};

/// Independent semantic history model owned by one Crux client.
#[derive(Debug, Default)]
pub struct Model {
    pub(super) chain: Option<String>,
    pub(super) filter: Filter,
    pub(super) selected: Selected,
    pub(super) expanded: BTreeSet<String>,
    pub(super) items: Vec<String>,
    pub(super) paging: Paging,
    pub(super) item_pages: BTreeMap<String, Paging>,
    pub(super) search: SearchView,
    pub(super) operation_details: BTreeMap<String, OperationDetailsState>,
    pub(super) operation_details_order: Vec<String>,
    pub(super) open: RequestState,
    pub(super) cache: Cache,
    pub(super) next_request: u64,
    pub(super) pending: BTreeMap<u64, QueryAction>,
}
