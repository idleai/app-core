//! Atomic replacement of loaded query windows after reconnect or invalidation.

use serde::{Deserialize, Serialize};

use super::{
    Filter, HistoryPage, Model, OperationDetails, Page, Paging, QueryAction, QueryResult,
    RequestState, SearchPage, SearchView, cache, reducer,
};
use crate::module::EffectError;

/// A logical item's loaded candidate window, rescanned from the beginning.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ItemScan {
    /// Full logical item identity.
    pub item: String,
    /// Number of bounded pages to replace, stopping early at the end of history.
    pub pages: u32,
}

/// One reconciliation read, evaluated against one refreshed engine index.
/// Page positions are transient scans, never reconnect checkpoints.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct Reconcile {
    /// Current history/search filter.
    pub filter: Filter,
    /// Current literal search, if any.
    pub text: String,
    /// Loaded history depth in bounded pages.
    pub history_pages: u32,
    /// Loaded search depth in bounded pages; zero when search is closed.
    pub search_pages: u32,
    /// Selected and expanded logical items, including their loaded depth.
    pub items: Vec<ItemScan>,
    /// Cached operation identities whose removal or quarantine must be checked.
    pub known: Vec<String>,
    /// Operations with open/cached details, whose late content must be re-read.
    pub details: Vec<String>,
}

/// Replacement candidate pages for one requested logical item.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ItemSnapshot {
    /// Requested item identity.
    pub item: String,
    /// Ordered candidate pages from the same refreshed index.
    pub pages: Vec<HistoryPage>,
}

/// Materialized replacement returned by one host read. No durable cursor is implied.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct Reconciled {
    /// Replacement loaded history window, beginning at the start of the chain.
    pub history: Vec<HistoryPage>,
    /// Replacement search matches and content gaps.
    pub search: Vec<SearchPage>,
    /// Replacement windows for the requested logical items.
    pub items: Vec<ItemSnapshot>,
    /// Every requested detail plus any missing/quarantined known operation.
    pub details: Vec<OperationDetails>,
}

fn pages(paging: &Paging) -> u32 {
    u32::try_from(paging.scanned.div_ceil(u64::from(Page::default().limit)))
        .unwrap_or(u32::MAX)
        .max(1)
}

pub(super) fn request(model: &Model) -> Reconcile {
    let mut items = model.expanded.clone();
    items.extend(model.item_pages.keys().cloned());
    items.extend(model.selected.item.iter().cloned());
    let mut details: std::collections::BTreeSet<_> =
        model.operation_details_order.iter().cloned().collect();
    details.extend(model.selected.observation.iter().cloned());
    Reconcile {
        filter: model.filter.clone(),
        text: model.search.text.clone(),
        history_pages: pages(&model.paging),
        search_pages: if model.search.text.is_empty() {
            0
        } else {
            pages(&model.search.paging)
        },
        items: items
            .into_iter()
            .map(|item| ItemScan {
                pages: model.item_pages.get(&item).map_or(1, pages),
                item,
            })
            .collect(),
        known: model.cache.identities(),
        details: details.into_iter().collect(),
    }
}

pub(super) fn apply(
    model: &mut Model,
    request: &Reconcile,
    result: Reconciled,
) -> Result<(), EffectError> {
    // Build and validate separately. A malformed final page cannot erase the
    // current view or leave half of a replacement visible.
    let mut staged = Model {
        chain: model.chain.clone(),
        filter: request.filter.clone(),
        selected: model.selected.clone(),
        expanded: model.expanded.clone(),
        search: SearchView {
            text: request.text.clone(),
            ..SearchView::default()
        },
        ..Model::default()
    };
    history_pages(&mut staged, request.history_pages, result.history, None)?;
    search_pages(&mut staged, request.search_pages, result.search)?;
    if result.items.len() != request.items.len() {
        return Err(cache::error(
            "Reconciliation omitted a requested logical item",
        ));
    }
    for (expected, item) in request.items.iter().zip(result.items) {
        if item.item != expected.item {
            return Err(cache::error(
                "Reconciliation returned a different logical item",
            ));
        }
        history_pages(&mut staged, expected.pages, item.pages, Some(&item.item))?;
    }
    let mut returned = std::collections::BTreeSet::new();
    for detail in result.details {
        let operation = detail.operation.clone();
        let requested = request.details.contains(&operation);
        if !returned.insert(operation.clone())
            || (!requested
                && (!request.known.contains(&operation)
                    || detail.status == super::RecordLookupStatus::Found))
        {
            return Err(cache::error(
                "Reconciliation returned unexpected operation details",
            ));
        }
        if detail.status != super::RecordLookupStatus::Found {
            // Keep a stable missing/conflicted anchor, but never its old content.
            if let Some(old) = model.cache.observation(&operation) {
                let _key = staged.cache.insert(old)?;
            }
        }
        reducer::apply(
            &mut staged,
            &QueryAction::OperationDetails {
                operation: operation.clone(),
            },
            QueryResult::OperationDetails(detail),
        )?;
        staged.operation_details_order.push(operation);
    }
    if request
        .details
        .iter()
        .any(|operation| !returned.contains(operation))
    {
        return Err(cache::error(
            "Reconciliation omitted requested operation details",
        ));
    }
    // Search position follows its record when lower IDs shift the match list.
    staged.search.cursor = model
        .search
        .cursor
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| model.search.matches.get(index))
        .and_then(|old| {
            staged
                .search
                .matches
                .iter()
                .position(|hit| hit.record == old.record)
        })
        .and_then(|index| u32::try_from(index).ok());
    model.cache = staged.cache;
    model.items = staged.items;
    model.paging = staged.paging;
    model.item_pages = staged.item_pages;
    model.search = staged.search;
    model.operation_details = staged.operation_details;
    model.operation_details_order = staged.operation_details_order;
    model.reconciliation = RequestState::Ready;
    Ok(())
}

fn history_pages(
    model: &mut Model,
    count: u32,
    pages: Vec<HistoryPage>,
    item: Option<&str>,
) -> Result<(), EffectError> {
    let length = pages.len();
    if length == 0 || u32::try_from(length).unwrap_or(u32::MAX) > count {
        return Err(cache::error(
            "Reconciliation returned an invalid page count",
        ));
    }
    let mut page = Page::default();
    for (index, result) in pages.into_iter().enumerate() {
        validate_end(index, length, count, result.next_after.as_deref())?;
        let action = item.map_or_else(
            || QueryAction::History {
                filter: model.filter.clone(),
                page: page.clone(),
            },
            |item| QueryAction::Item {
                item: item.to_owned(),
                page: page.clone(),
            },
        );
        page.after.clone_from(&result.next_after);
        reducer::apply(model, &action, QueryResult::History(result))?;
    }
    Ok(())
}

fn search_pages(model: &mut Model, count: u32, pages: Vec<SearchPage>) -> Result<(), EffectError> {
    let length = pages.len();
    if u32::try_from(length).unwrap_or(u32::MAX) > count || (count > 0 && length == 0) {
        return Err(cache::error(
            "Reconciliation returned an invalid search page count",
        ));
    }
    let mut page = Page::default();
    for (index, result) in pages.into_iter().enumerate() {
        validate_end(index, length, count, result.next_after.as_deref())?;
        let action = QueryAction::Search {
            text: model.search.text.clone(),
            filter: model.filter.clone(),
            page: page.clone(),
        };
        page.after.clone_from(&result.next_after);
        reducer::apply(model, &action, QueryResult::Search(result))?;
    }
    Ok(())
}

fn validate_end(
    index: usize,
    length: usize,
    count: u32,
    after: Option<&str>,
) -> Result<(), EffectError> {
    let last = index.saturating_add(1) == length;
    if (!last && after.is_none())
        || (last && u32::try_from(length).ok() != Some(count) && after.is_some())
    {
        return Err(cache::error(
            "Reconciliation stopped before its requested boundary",
        ));
    }
    Ok(())
}
