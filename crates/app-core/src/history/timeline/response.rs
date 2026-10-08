//! Correlated results, byte-bounded caching and readable failure states.

use std::collections::BTreeMap;

use crux_core::{Command, render};
use idle_history::timeline::{
    Action, Cursor, DEFAULT_LIMIT, MAX_CACHED_BYTES, MAX_CACHED_ROWS, MAX_LIMIT, Position,
    Response, VERSION, Window,
};

use super::{
    model::{Model, State, Surface},
    reducer::{TimelineCommand, error, move_row, navigate_match, request, window},
};
use crate::{
    history::{QueryAction, QueryOutput, QueryResult, RequestState},
    module::EffectError,
};

pub(super) fn complete(model: &mut Model, id: u64, result: QueryOutput) -> TimelineCommand {
    let Some((pending, _exclusive)) = model.pending.take(id) else {
        return Command::done();
    };
    if matches!(pending.query, QueryAction::OpenAt { .. }) {
        model.open = match result {
            Ok(QueryResult::Opened) => RequestState::Ready,
            Ok(_) => RequestState::Failed(error("The host returned a different native action.")),
            Err(error) => RequestState::Failed(error),
        };
        return render::render();
    }
    let QueryAction::Timeline(query) = pending.query else {
        return Command::done();
    };
    let result = result.and_then(|result| {
        if let QueryResult::Timeline(response) = result {
            Ok(*response)
        } else {
            Err(error("The host does not support indexed Activity history."))
        }
    });
    let response = match result {
        Ok(response) => response,
        Err(error) => {
            fail(model, pending.surface, &query.action, error);
            return render::render();
        }
    };
    match response {
        Response::Building(progress) => {
            let state = model.surfaces.entry(pending.surface).or_default();
            if !matches!(query.action, Action::Advance { .. }) {
                state.resume = Some(query.action);
            }
            state.view.progress = Some(progress.clone());
            request(
                model,
                pending.surface,
                Action::Advance {
                    build: progress.build,
                },
            )
        }
        Response::Window(value) => {
            if matches!(query.action, Action::Advance { .. }) {
                let state = model.surfaces.entry(pending.surface).or_default();
                state.view.progress = None;
                let next = state.resume.take().or_else(|| state.intent.clone());
                return next
                    .map_or_else(render::render, |next| request(model, pending.surface, next));
            }
            if let Err(error) = validate(&value) {
                fail(model, pending.surface, &query.action, error);
                return render::render();
            }
            if let Action::Window {
                position: Position::Seek(id),
                ..
            } = &query.action
                && let Some(selection) = model
                    .selected
                    .as_ref()
                    .filter(|selection| selection.occurrence == *id)
                && !value
                    .rows
                    .iter()
                    .any(|row| row.occurrence == *id && row.address == selection.address)
            {
                fail(
                    model,
                    pending.surface,
                    &query.action,
                    error("The selected Activity record is no longer available in this source."),
                );
                return render::render();
            }
            let state = model.surfaces.entry(pending.surface).or_default();
            if let Action::Window {
                position: Position::Seek(id),
                ..
            } = &query.action
                && let Some(group) = value
                    .rows
                    .iter()
                    .find(|row| row.occurrence == *id)
                    .and_then(|row| row.group.as_ref())
                    .filter(|group| group.expanded)
            {
                let _inserted = state.temporary.insert(group.id.clone());
            }
            if protect_reading(state, &value) {
                let anchor = state.anchor.clone();
                return window(model, pending.surface, Position::Refresh(anchor));
            }
            let rebuilding = value.rebuilding.clone();
            if state
                .view
                .window
                .as_ref()
                .is_some_and(|old| value.activities > old.activities)
                && !state.at_newest
            {
                state.view.new_activity = true;
            }
            if let Err(error) = install(state, &query.action, value) {
                fail(model, pending.surface, &query.action, error);
                return render::render();
            }
            if let Some(progress) = rebuilding {
                state.resume = Some(query.action);
                state.view.progress = Some(progress.clone());
                return request(
                    model,
                    pending.surface,
                    Action::Advance {
                        build: progress.build,
                    },
                );
            }
            if let Some(delta) = state.pending_move.take() {
                return move_row(model, pending.surface, delta);
            }
            render::render()
        }
        Response::Found(found) => {
            let Action::Find {
                cursor,
                text,
                limit,
                ..
            } = &query.action
            else {
                fail(
                    model,
                    pending.surface,
                    &query.action,
                    error("Unexpected Activity Find response."),
                );
                return render::render();
            };
            if found.matches.len() > usize::try_from(*limit).unwrap_or(usize::MAX)
                || found.total < u64::try_from(found.matches.len()).unwrap_or(u64::MAX)
                || found.matches.iter().any(|hit| {
                    hit.preview.len() > idle_history::MAX_ROW_TEXT_BYTES
                        || hit.occurrence.len() > 256
                        || hit.group.as_ref().is_some_and(|id| id.len() > 512)
                        || !hit.address.is_valid()
                })
            {
                fail(
                    model,
                    pending.surface,
                    &query.action,
                    error("The host exceeded the Activity match limit."),
                );
                return render::render();
            }
            let state = model.surfaces.entry(pending.surface).or_default();
            if state.view.search.text != *text {
                return Command::done();
            }
            state.view.search.state = RequestState::Ready;
            state.view.search.matches = found.matches;
            state.view.search.offset = cursor.as_ref().map_or(0, |cursor| cursor.offset);
            state.view.search.total = found.total;
            state.view.search.next = found.next.or_else(|| cursor.clone());
            state.view.search.unavailable = found.unavailable;
            if let Err(error) = bound(state, None) {
                fail(model, pending.surface, &query.action, error);
                return render::render();
            }
            navigate_match(model, pending.surface, 0).and(render::render())
        }
        Response::Stale => {
            let state = model.surfaces.entry(pending.surface).or_default();
            match query.action {
                Action::Find {
                    view, text, limit, ..
                } => {
                    state.view.search.current = None;
                    request(
                        model,
                        pending.surface,
                        Action::Find {
                            view,
                            text,
                            cursor: None,
                            limit,
                        },
                    )
                }
                Action::Window { .. }
                | Action::Members { .. }
                | Action::Advance { .. }
                | Action::Cancel { .. } => {
                    let anchor = state.anchor.clone();
                    window(model, pending.surface, Position::Refresh(anchor))
                }
            }
        }
        Response::Cancelled => {
            let state = model.surfaces.entry(pending.surface).or_default();
            state.view.progress = None;
            state.view.state = RequestState::Idle;
            render::render()
        }
    }
}

fn fail(model: &mut Model, surface: Surface, action: &Action, failure: EffectError) {
    let state = model.surfaces.entry(surface).or_default();
    if matches!(action, Action::Find { .. }) {
        state.view.search.state = RequestState::Failed(failure);
    } else {
        state.view.state = RequestState::Failed(failure);
        state.view.progress = None;
    }
}

fn validate(window: &Window) -> Result<(), EffectError> {
    if window.version != VERSION || window.revision.is_empty() {
        return Err(error(
            "The Activity host returned an unsupported timeline format.",
        ));
    }
    if window.rows.len() > usize::try_from(MAX_LIMIT).unwrap_or(usize::MAX) {
        return Err(error("The host exceeded the Activity row limit."));
    }
    for row in &window.rows {
        if row.occurrence.is_empty()
            || row.occurrence.len() > 256
            || row.group.as_ref().is_some_and(|group| group.id.len() > 512)
            || !row.address.is_valid()
            || row.title.len() > idle_history::MAX_TOOL_LABEL_BYTES
            || row.preview.len() > idle_history::MAX_ROW_TEXT_BYTES
        {
            return Err(error("The host returned an invalid Activity row."));
        }
    }
    let bytes = serde_json::to_vec(window)
        .map_err(|cause| error(format!("Activity window serialization failed: {cause}")))?
        .len();
    if bytes > MAX_CACHED_BYTES / 2 {
        return Err(error("The Activity window exceeds the client byte limit."));
    }
    Ok(())
}

fn protect_reading(state: &mut State, value: &Window) -> bool {
    let mut retry = false;
    for group in value.rows.iter().filter_map(|row| row.group.as_ref()) {
        if !group.live
            && !group.expanded
            && state.visible.contains(&group.id)
            && !state.manual.contains_key(&group.id)
            && state
                .view
                .window
                .as_ref()
                .into_iter()
                .flat_map(|window| &window.rows)
                .filter_map(|row| row.group.as_ref())
                .any(|old| old.id == group.id && old.expanded)
        {
            retry |= state.reading.insert(group.id.clone());
        }
    }
    retry
}

fn install(state: &mut State, action: &Action, mut value: Window) -> Result<(), EffectError> {
    let cursor = if let Action::Window {
        position: Position::Page(cursor),
        ..
    } = action
    {
        Some(cursor)
    } else {
        None
    };
    let mut trim_newer = true;
    if let Some(cursor) = cursor
        && let Some(old) = &state.view.window
        && old.revision == value.revision
    {
        let old_end = old
            .offset
            .saturating_add(u64::try_from(old.rows.len()).unwrap_or(u64::MAX));
        let new_end = value
            .offset
            .saturating_add(u64::try_from(value.rows.len()).unwrap_or(u64::MAX));
        if value.offset <= old_end && new_end >= old.offset {
            let mut rows = BTreeMap::new();
            for (index, row) in old.rows.iter().enumerate() {
                drop(
                    rows.insert(
                        old.offset
                            .saturating_add(u64::try_from(index).unwrap_or(u64::MAX)),
                        row.clone(),
                    ),
                );
            }
            for (index, row) in value.rows.drain(..).enumerate() {
                drop(
                    rows.insert(
                        value
                            .offset
                            .saturating_add(u64::try_from(index).unwrap_or(u64::MAX)),
                        row,
                    ),
                );
            }
            trim_newer = cursor.offset >= old.offset;
            if value.offset > old.offset {
                value.offset = old.offset;
                value.newer.clone_from(&old.newer);
            }
            if new_end < old_end {
                value.older.clone_from(&old.older);
            }
            value.rows = rows.into_values().collect();
        }
    }
    state.view.window = Some(value);
    state.view.state = RequestState::Ready;
    state.view.progress = None;
    bound(state, cursor.map(|cursor| (cursor, trim_newer)))
}

fn bound(state: &mut State, paging: Option<(&Cursor, bool)>) -> Result<(), EffectError> {
    let maximum = MAX_CACHED_BYTES / 2 - 1_048_576;
    loop {
        let bytes = serde_json::to_vec(&(&state.view.window, &state.view.search))
            .map_err(|cause| error(format!("Activity cache serialization failed: {cause}")))?
            .len();
        let rows = state
            .view
            .window
            .as_ref()
            .map_or(0, |window| window.rows.len());
        if bytes <= maximum
            && rows.saturating_add(state.view.search.matches.len()) <= MAX_CACHED_ROWS / 2
        {
            state.view.cached_bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
            return Ok(());
        }
        let Some(window) = state
            .view
            .window
            .as_mut()
            .filter(|window| window.rows.len() > 1)
        else {
            return Err(error("The Activity data exceeds the client cache limit."));
        };
        let trim_newer = paging.is_none_or(|(_, newer)| newer);
        if trim_newer {
            let remove = window
                .rows
                .len()
                .min(usize::try_from(DEFAULT_LIMIT).unwrap_or(200))
                .min(window.rows.len().saturating_sub(1));
            let _removed: Vec<_> = window.rows.drain(..remove).collect();
            window.offset = window
                .offset
                .saturating_add(u64::try_from(remove).unwrap_or(u64::MAX));
            if let Some((cursor, _)) = paging {
                window.newer = Some(Cursor {
                    offset: window.offset.saturating_sub(u64::from(DEFAULT_LIMIT)),
                    ..cursor.clone()
                });
            }
        } else {
            window.rows.truncate(
                window
                    .rows
                    .len()
                    .saturating_sub(usize::try_from(DEFAULT_LIMIT).unwrap_or(200))
                    .max(1),
            );
            if let Some((cursor, _)) = paging {
                window.older = Some(Cursor {
                    offset: window
                        .offset
                        .saturating_add(u64::try_from(window.rows.len()).unwrap_or(u64::MAX)),
                    ..cursor.clone()
                });
            }
        }
    }
}
