//! Crux transitions and host request continuations for semantic history.

use crux_core::{App, Command, render};
use serde::{Deserialize, Serialize};

use super::{
    Filter, HistoryPage, MatchView, Model, OpenTarget, OperationDetailsState, OperationDetailsView,
    Page, Paging, Query, QueryAction, QueryOutput, QueryResult, RecordLookupStatus, RecordRef,
    RequestState, SearchView, Selected, ViewModel, cache, reconciliation,
};
use crate::{effects::Effect, module::EffectError};

use HistoryEvent as Event;

type HistoryCommand = Command<Effect, Event>;
const MAX_CACHED_OPERATION_DETAILS: usize = 64;

/// Client history actions, with results accepted only through Crux continuations.
#[derive(Clone, Debug, Deserialize, Serialize, facet::Facet)]
#[repr(u8)]
pub enum HistoryEvent {
    /// Select a chain and begin a fresh history scan; another chain clears caches.
    Connect(String),
    /// Detach from the chain and clear all chain-scoped state.
    Disconnect,
    /// Apply conjunctive recorded-fact filters and restart history/search scans.
    SetFilter(Filter),
    /// Load the next candidate page, or retry a failed page.
    LoadMore,
    /// Start an exact literal search. Empty text clears the search session.
    Search(String),
    /// Continue search through candidate pages, including pages without matches.
    SearchMore,
    /// Move the match cursor with wrapping, selecting the stable destination.
    NavigateMatch(i32),
    /// Select a logical object and optionally one of its recorded operations.
    Select(Selected),
    /// Clear selection without discarding cached observations.
    ClearSelection,
    /// Toggle disclosure by logical identity. Expanding begins an item scan.
    ToggleDisclosure(String),
    /// Continue or retry the bounded scan of all observations for an item.
    LoadItem(String),
    /// Load an operation's records/content, retry a failure, or refresh late content.
    LoadOperationDetails {
        /// Full observation identity.
        operation: String,
        /// Refresh even when the operation's records/content are already cached.
        refresh: bool,
    },
    /// Re-scan from the beginning, retaining selection and disclosure identities.
    Refresh,
    /// Request a host-native action using an operation ID and stored-record digest.
    Open {
        /// Retained record identity.
        record: RecordRef,
        /// Platform action.
        target: OpenTarget,
    },
    /// Retire in-flight reads when a transport closes, preserving visible state.
    Suspend,
    /// Read a fresh snapshot on a new connection, preserving interaction state.
    Reconnect,
    /// Internal continuation. Serialized client events cannot forge results.
    #[serde(skip)]
    #[facet(skip)]
    Completed {
        /// Locally allocated continuation token.
        request: u64,
        /// Typed result returned by the host.
        result: QueryOutput,
    },
}

/// Shared history domain reducer with no platform I/O.
#[derive(Debug, Default)]
pub struct History;

impl App for History {
    type Event = Event;
    type Model = Model;
    type ViewModel = ViewModel;
    type Effect = Effect;

    fn update(&self, event: Event, model: &mut Model) -> HistoryCommand {
        let restart = model.reconciliation == RequestState::Loading
            && matches!(
                &event,
                Event::SetFilter(_)
                    | Event::Search(_)
                    | Event::NavigateMatch(_)
                    | Event::Select(_)
                    | Event::ClearSelection
                    | Event::ToggleDisclosure(_)
                    | Event::LoadItem(_)
                    | Event::LoadOperationDetails { .. }
            );
        let command = transition(event, model);
        if restart {
            suspend(model);
            command.and(refresh(model))
        } else {
            command
        }
    }

    fn view(&self, model: &Model) -> ViewModel {
        ViewModel {
            chain: model.chain.clone(),
            filter: model.filter.clone(),
            selected: model.selected.clone(),
            expanded: model.expanded.iter().cloned().collect(),
            items: model
                .items
                .iter()
                .filter_map(|key| model.cache.item(key, model))
                .collect(),
            selected_item: model
                .selected
                .item
                .as_ref()
                .and_then(|key| model.cache.item(key, model)),
            paging: model.paging.clone(),
            search: model.search.clone(),
            operation_details: model
                .operation_details
                .iter()
                .map(|(operation, state)| OperationDetailsView {
                    operation: operation.clone(),
                    state: state.clone(),
                })
                .collect(),
            open: model.open.clone(),
            cache: model.cache.status(),
            reconciliation: model.reconciliation.clone(),
        }
    }
}

fn transition(event: Event, model: &mut Model) -> HistoryCommand {
    match event {
        Event::Connect(chain) => {
            if model.chain.as_ref() == Some(&chain) {
                return Command::done();
            }
            reset(model);
            model.chain = Some(chain);
            load_history(model)
        }
        Event::Disconnect => {
            reset(model);
            render::render()
        }
        Event::SetFilter(filter) => {
            if model.filter == filter {
                return Command::done();
            }
            model.filter = filter;
            restart_scans(model)
        }
        Event::LoadMore => load_history(model),
        Event::Search(text) => {
            model
                .pending
                .retain(|action| !matches!(action, QueryAction::Search { .. }));
            model.search = SearchView {
                text,
                ..SearchView::default()
            };
            load_search(model).and(render::render())
        }
        Event::SearchMore => load_search(model),
        Event::NavigateMatch(delta) => navigate(model, delta),
        Event::Select(selected) => select(model, selected),
        Event::ClearSelection => {
            model.selected = Selected::default();
            render::render()
        }
        Event::ToggleDisclosure(item) => {
            if cache::full_id(&item).is_err() {
                return Command::done();
            }
            if model.expanded.remove(&item) {
                return render::render();
            }
            let _inserted = model.expanded.insert(item.clone());
            load_item(model, item).and(render::render())
        }
        Event::LoadItem(item) => load_item(model, item),
        Event::LoadOperationDetails { operation, refresh } => {
            load_operation_details(model, operation, refresh)
        }
        Event::Refresh => refresh(model),
        Event::Suspend => {
            suspend(model);
            render::render()
        }
        Event::Reconnect => {
            suspend(model);
            refresh(model)
        }
        Event::Open { record, target } => {
            if model.chain.is_none() {
                model.open = RequestState::Failed(cache::error("No history chain is selected"));
                return render::render();
            }
            if cache::full_id(&record.operation).is_err() || cache::full_id(&record.hash).is_err() {
                model.open = RequestState::Failed(cache::error(
                    "Native action requires a full operation ID and record digest",
                ));
                return render::render();
            }
            model
                .pending
                .retain(|action| !matches!(action, QueryAction::Open { .. }));
            model.open = RequestState::Loading;
            request(model, QueryAction::Open { record, target })
        }
        Event::Completed { request, result } => complete(model, request, result),
    }
}

fn reset(model: &mut Model) {
    // Never reuse a token from a detached context, even when switching A -> B -> A.
    *model = Model {
        pending: {
            model.pending.clear();
            std::mem::take(&mut model.pending)
        },
        ..Model::default()
    };
}

fn request(model: &mut Model, action: QueryAction) -> HistoryCommand {
    if model.reconciliation == RequestState::Loading
        && !matches!(action, QueryAction::Reconcile(_) | QueryAction::Open { .. })
    {
        return Command::done();
    }
    let Some(chain) = model.chain.clone() else {
        return Command::done();
    };
    if model.pending.values().any(|pending| *pending == action) {
        return Command::done();
    }
    let Ok(id) = model.pending.register(action.clone(), false) else {
        fail(
            model,
            &action,
            cache::error("History request identity exhausted"),
        );
        return render::render();
    };
    Command::request_from_shell(Query { chain, action })
        .then_send(move |result| Event::Completed {
            request: id,
            result,
        })
        .and(render::render())
}

fn next_page(paging: &Paging) -> Option<Page> {
    if paging.exhausted || paging.state == RequestState::Loading {
        return None;
    }
    Some(Page {
        after: paging.next_after.clone(),
        ..Page::default()
    })
}

fn load_history(model: &mut Model) -> HistoryCommand {
    if model.reconciliation == RequestState::Loading {
        return Command::done();
    }
    if model.chain.is_none() {
        return Command::done();
    }
    let Some(page) = next_page(&model.paging) else {
        return Command::done();
    };
    model.paging.state = RequestState::Loading;
    request(
        model,
        QueryAction::History {
            filter: model.filter.clone(),
            page,
        },
    )
}

fn load_search(model: &mut Model) -> HistoryCommand {
    if model.reconciliation == RequestState::Loading {
        return Command::done();
    }
    if model.chain.is_none() || model.search.text.is_empty() {
        return Command::done();
    }
    if model.search.text.len() > 16_384 {
        model.search.paging.state =
            RequestState::Failed(cache::error("Search text exceeds 16384 UTF-8 bytes"));
        return render::render();
    }
    let Some(page) = next_page(&model.search.paging) else {
        return Command::done();
    };
    model.search.paging.state = RequestState::Loading;
    request(
        model,
        QueryAction::Search {
            text: model.search.text.clone(),
            filter: model.filter.clone(),
            page,
        },
    )
}

fn load_item(model: &mut Model, item: String) -> HistoryCommand {
    if model.chain.is_none() || cache::full_id(&item).is_err() {
        return Command::done();
    }
    let paging = model.item_pages.entry(item.clone()).or_default();
    let Some(page) = next_page(paging) else {
        return Command::done();
    };
    paging.state = RequestState::Loading;
    request(model, QueryAction::Item { item, page })
}

fn load_operation_details(model: &mut Model, operation: String, refresh: bool) -> HistoryCommand {
    if model.chain.is_none() {
        return Command::done();
    }
    if !refresh
        && matches!(
            model.operation_details.get(&operation),
            Some(OperationDetailsState::Ready(_) | OperationDetailsState::Loading)
        )
    {
        return Command::done();
    }
    model.pending.retain(|action| !matches!(action, QueryAction::OperationDetails { operation: pending } if *pending == operation));
    if let Err(error) = cache::full_id(&operation) {
        let _previous = model
            .operation_details
            .insert(operation, OperationDetailsState::Failed(error));
        return render::render();
    }
    let _previous = model
        .operation_details
        .insert(operation.clone(), OperationDetailsState::Loading);
    model.operation_details_order.retain(|id| *id != operation);
    model.operation_details_order.push(operation.clone());
    request(model, QueryAction::OperationDetails { operation })
}

fn restart_scans(model: &mut Model) -> HistoryCommand {
    model.pending.retain(|action| {
        !matches!(
            action,
            QueryAction::History { .. } | QueryAction::Search { .. }
        )
    });
    model.items.clear();
    model.paging = Paging::default();
    model.search = SearchView {
        text: model.search.text.clone(),
        ..SearchView::default()
    };
    load_history(model)
        .and(load_search(model))
        .and(render::render())
}

fn suspend(model: &mut Model) {
    model.pending.clear();
    model.reconcile_again = false;
    model.reconciliation = RequestState::Idle;
    if model.paging.state == RequestState::Loading {
        model.paging.state = RequestState::Idle;
    }
    if model.search.paging.state == RequestState::Loading {
        model.search.paging.state = RequestState::Idle;
    }
    for page in model.item_pages.values_mut() {
        if page.state == RequestState::Loading {
            page.state = RequestState::Idle;
        }
    }
    model
        .operation_details
        .retain(|_, value| !matches!(value, OperationDetailsState::Loading));
    if model.open == RequestState::Loading {
        model.open = RequestState::Idle;
    }
}

fn refresh(model: &mut Model) -> HistoryCommand {
    if model.chain.is_none() {
        return Command::done();
    }
    if model.reconciliation == RequestState::Loading {
        model.reconcile_again = true;
        return Command::done();
    }
    let snapshot = reconciliation::request(model);
    suspend(model);
    model.reconciliation = RequestState::Loading;
    request(model, QueryAction::Reconcile(Box::new(snapshot)))
}

fn select(model: &mut Model, selected: Selected) -> HistoryCommand {
    if selected
        .item
        .as_deref()
        .is_some_and(|id| cache::full_id(id).is_err())
        || selected
            .observation
            .as_deref()
            .is_some_and(|id| cache::full_id(id).is_err())
    {
        return Command::done();
    }
    if let Some(operation) = &selected.observation
        && let Some(item) = model.cache.item_for_observation(operation)
        && selected.item.as_ref() != Some(&item)
    {
        return Command::done();
    }
    model.selected = selected;
    let mut command = render::render();
    if let Some(item) = model.selected.item.clone()
        && !model.item_pages.contains_key(&item)
    {
        command = command.and(load_item(model, item));
    }
    if let Some(operation) = model.selected.observation.clone() {
        command = command.and(load_operation_details(model, operation, false));
    }
    command
}

fn navigate(model: &mut Model, delta: i32) -> HistoryCommand {
    let len = model.search.matches.len();
    if len == 0 {
        return Command::done();
    }
    let step = usize::try_from(delta.unsigned_abs())
        .unwrap_or(usize::MAX)
        .rem_euclid(len);
    let index = model
        .search
        .cursor
        .and_then(|index| usize::try_from(index).ok());
    let index = index.map_or_else(
        || if delta < 0 { len.saturating_sub(1) } else { 0 },
        |index| {
            if delta < 0 {
                index
                    .saturating_add(len)
                    .saturating_sub(step)
                    .rem_euclid(len)
            } else {
                index.saturating_add(step).rem_euclid(len)
            }
        },
    );
    let Some(hit) = model.search.matches.get(index) else {
        return Command::done();
    };
    let selected = Selected {
        item: Some(hit.item.clone()),
        observation: Some(hit.record.operation.clone()),
    };
    model.search.cursor = u32::try_from(index).ok();
    let _expanded = model.expanded.insert(hit.item.clone());
    select(model, selected)
}

fn complete(model: &mut Model, id: u64, result: QueryOutput) -> HistoryCommand {
    let Some((action, _window)) = model.pending.take(id) else {
        return Command::done();
    };
    if let Err(error) = result.and_then(|result| apply(model, &action, result)) {
        fail(model, &action, error);
    }
    let evicted = model
        .cache
        .prune(model.selected.item.as_deref(), &model.expanded);
    for item in evicted {
        let _old = model.item_pages.remove(&item);
    }
    prune_operation_details(model);
    if matches!(action, QueryAction::Reconcile(_)) && model.reconcile_again {
        model.reconcile_again = false;
        return refresh(model);
    }
    render::render()
}

fn settle(paging: &mut Paging, scanned: u32, next_after: Option<String>) {
    paging.state = RequestState::Ready;
    paging.scanned = paging.scanned.saturating_add(u64::from(scanned));
    paging.exhausted = next_after.is_none();
    paging.next_after = next_after;
}

fn validate_page(request: &Page, scanned: u32, cursor: Option<&str>) -> Result<(), EffectError> {
    if scanned > request.limit {
        return Err(cache::error("Host exceeded the candidate page limit"));
    }
    if let Some(cursor) = cursor {
        let cursor = cache::full_id(cursor)?;
        if scanned == 0
            || request
                .after
                .as_deref()
                .map(cache::full_id)
                .transpose()?
                .is_some_and(|after| cursor <= after)
        {
            return Err(cache::error("Host returned a non-advancing page cursor"));
        }
    }
    Ok(())
}

fn validate_history(action: &QueryAction, result: &HistoryPage) -> Result<(), EffectError> {
    let request = match action {
        QueryAction::History { page, .. } | QueryAction::Item { page, .. } => page,
        QueryAction::Search { .. }
        | QueryAction::OperationDetails { .. }
        | QueryAction::Open { .. }
        | QueryAction::Reconcile(_) => {
            return Err(cache::error("Wrong history response type"));
        }
    };
    validate_page(request, result.scanned, result.next_after.as_deref())?;
    if u32::try_from(result.observations.len()).unwrap_or(u32::MAX) > result.scanned {
        return Err(cache::error("More observations than inspected candidates"));
    }
    let mut previous = request.after.as_deref().map(cache::full_id).transpose()?;
    let cursor = result
        .next_after
        .as_deref()
        .map(cache::full_id)
        .transpose()?;
    for value in &result.observations {
        let op = value.operation()?;
        if previous.is_some_and(|previous| op.id <= previous)
            || cursor.is_some_and(|cursor| op.id > cursor)
        {
            return Err(cache::error(
                "Observation falls outside the requested candidate page",
            ));
        }
        previous = Some(op.id);
        let matches = match action {
            QueryAction::History { filter, .. } => filter.matches(&op),
            QueryAction::Item { item, .. } => value.item_key()? == *item,
            QueryAction::Search { .. }
            | QueryAction::OperationDetails { .. }
            | QueryAction::Open { .. }
            | QueryAction::Reconcile(_) => false,
        };
        if !matches {
            return Err(cache::error("History response does not match its query"));
        }
    }
    Ok(())
}

pub(super) fn apply(
    model: &mut Model,
    action: &QueryAction,
    result: QueryResult,
) -> Result<(), EffectError> {
    match (action, result) {
        (QueryAction::History { .. } | QueryAction::Item { .. }, QueryResult::History(result)) => {
            validate_history(action, &result)?;
            for observation in result.observations {
                let key = model.cache.insert(observation)?;
                if matches!(action, QueryAction::History { .. }) && !model.items.contains(&key) {
                    model.items.push(key);
                }
            }
            let paging = match action {
                QueryAction::Item { item, .. } => model.item_pages.entry(item.clone()).or_default(),
                QueryAction::History { .. } => &mut model.paging,
                QueryAction::Search { .. }
                | QueryAction::OperationDetails { .. }
                | QueryAction::Open { .. }
                | QueryAction::Reconcile(_) => return Err(cache::error("Wrong history request")),
            };
            settle(paging, result.scanned, result.next_after);
        }
        (QueryAction::Search { page, filter, .. }, QueryResult::Search(result)) => {
            validate_page(page, result.scanned, result.next_after.as_deref())?;
            if u32::try_from(result.matches.len()).unwrap_or(u32::MAX) > result.scanned {
                return Err(cache::error("More search hits than inspected candidates"));
            }
            let mut previous = page.after.as_deref().map(cache::full_id).transpose()?;
            let cursor = result
                .next_after
                .as_deref()
                .map(cache::full_id)
                .transpose()?;
            for hit in &result.matches {
                let operation = hit.observation.operation()?;
                if previous.is_some_and(|previous| operation.id <= previous)
                    || cursor.is_some_and(|cursor| operation.id > cursor)
                {
                    return Err(cache::error(
                        "Search hit falls outside the requested candidate page",
                    ));
                }
                previous = Some(operation.id);
                if !filter.matches(&operation)
                    || hit.fields.iter().any(|field| field.start > field.end)
                {
                    return Err(cache::error("Search response does not match its query"));
                }
            }
            for hit in result.matches {
                let item = model.cache.insert(hit.observation.clone())?;
                if !model
                    .search
                    .matches
                    .iter()
                    .any(|value| value.record == hit.observation.record)
                {
                    model.search.matches.push(MatchView {
                        item,
                        record: hit.observation.record,
                        fields: hit.fields,
                    });
                }
            }
            for gap in result.unavailable {
                if !model
                    .search
                    .unavailable
                    .iter()
                    .any(|value| value.record == gap.record && value.field == gap.field)
                {
                    model.search.unavailable.push(gap);
                }
            }
            settle(&mut model.search.paging, result.scanned, result.next_after);
        }
        (QueryAction::OperationDetails { operation }, QueryResult::OperationDetails(result))
            if *operation == result.operation =>
        {
            for record in &result.records {
                let _hash = cache::full_id(&record.record.hash)?;
                if record.record.operation != *operation {
                    return Err(cache::error(
                        "Returned record contains a different operation ID",
                    ));
                }
            }
            match result.status {
                RecordLookupStatus::Found => {
                    let observation = result.observation.as_ref().ok_or_else(|| {
                        cache::error("Lookup result lacks its accepted operation")
                    })?;
                    if observation.record.operation != *operation
                        || result.records.len() != 1
                        || result
                            .fields
                            .iter()
                            .any(|field| field.record != observation.record)
                        || !result
                            .records
                            .iter()
                            .any(|record| record.record == observation.record)
                    {
                        return Err(cache::error(
                            "Lookup result lacks the operation's original stored bytes",
                        ));
                    }
                    let _key = model.cache.insert(observation.clone())?;
                }
                RecordLookupStatus::Missing | RecordLookupStatus::Conflicted => {
                    if result.observation.is_some()
                        || !result.fields.is_empty()
                        || result.comparison.is_some()
                        || (result.status == RecordLookupStatus::Missing
                            && !result.records.is_empty())
                        || (result.status == RecordLookupStatus::Conflicted
                            && result.records.is_empty())
                    {
                        return Err(cache::error(
                            "Missing or conflicted operations cannot contain an accepted record or resolved content",
                        ));
                    }
                    model
                        .cache
                        .mark_unavailable(operation, result.status == RecordLookupStatus::Missing);
                    // Previously matched representations are no longer accepted facts.
                    model
                        .search
                        .matches
                        .retain(|hit| hit.record.operation != *operation);
                    model.search.cursor = None;
                }
            }
            let _old = model
                .operation_details
                .insert(operation.clone(), OperationDetailsState::Ready(result));
        }
        (QueryAction::Open { .. }, QueryResult::Opened) => model.open = RequestState::Ready,
        (QueryAction::Reconcile(request), QueryResult::Reconciled(result)) => {
            reconciliation::apply(model, request, *result)?;
        }
        _ => {
            return Err(cache::error(
                "Host returned a result for a different history operation",
            ));
        }
    }
    Ok(())
}

fn fail(model: &mut Model, action: &QueryAction, error: EffectError) {
    match action {
        QueryAction::History { .. } => model.paging.state = RequestState::Failed(error),
        QueryAction::Search { .. } => model.search.paging.state = RequestState::Failed(error),
        QueryAction::Item { item, .. } => {
            model.item_pages.entry(item.clone()).or_default().state = RequestState::Failed(error);
        }
        QueryAction::OperationDetails { operation } => {
            let _old = model
                .operation_details
                .insert(operation.clone(), OperationDetailsState::Failed(error));
        }
        QueryAction::Open { .. } => model.open = RequestState::Failed(error),
        QueryAction::Reconcile(_) => model.reconciliation = RequestState::Failed(error),
    }
}

fn prune_operation_details(model: &mut Model) {
    let mut remove = model
        .operation_details
        .len()
        .saturating_sub(MAX_CACHED_OPERATION_DETAILS);
    model.operation_details_order.retain(|operation| {
        if remove == 0
            || model.selected.observation.as_ref() == Some(operation)
            || matches!(
                model.operation_details.get(operation),
                Some(OperationDetailsState::Loading)
            )
        {
            return true;
        }
        let _removed = model.operation_details.remove(operation);
        remove = remove.saturating_sub(1);
        false
    });
}
