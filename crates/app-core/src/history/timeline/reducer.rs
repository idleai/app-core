//! Activity requests, disclosure and exact native activation.

use crux_core::{Command, render};
use idle_history::timeline::{Action, DEFAULT_LIMIT, Position, Request, VERSION};
use serde::{Deserialize, Serialize};

use super::{
    model::{Model, Pending, Selection, Surface},
    response,
};
use crate::{
    effects::Effect,
    history::{Filter, OpenTarget, Query, QueryAction, QueryOutput, RequestState},
    module::EffectError,
};

pub(super) type TimelineCommand = Command<Effect, Event>;

/// Semantic Activity actions accepted from either composition.
#[derive(Clone, Debug, Deserialize, Serialize, facet::Facet)]
#[repr(u8)]
#[facet(rename = "TimelineEvent")]
pub enum Event {
    /// Initialize this composition's bounded latest window.
    Load(Surface),
    /// Seek an exact occurrence without opening a native editor.
    Seek {
        /// Target composition.
        surface: Surface,
        /// Stable occurrence identity.
        occurrence: String,
    },
    /// Reveal the exact selection transferred from another composition.
    Reveal {
        /// Destination composition.
        surface: Surface,
        /// Exact occurrence and recorded source address.
        selection: Selection,
    },
    /// Read the adjacent window in either direction.
    Page {
        /// Target composition.
        surface: Surface,
        /// True moves toward newer activities.
        newer: bool,
    },
    /// Refresh while retaining the semantic viewport anchor.
    Refresh(Surface),
    /// Follow the newest activity and clear the new-activity indicator.
    Latest(Surface),
    /// Change recorded filters while retaining manual disclosure choices.
    Filter {
        /// Target composition.
        surface: Surface,
        /// Conjunctive recorded fields.
        filter: Filter,
    },
    /// Select or activate an exact row already supplied by the host.
    Select {
        /// Composition containing the row.
        surface: Surface,
        /// Exact occurrence identity.
        occurrence: String,
        /// Open the native editor after selection.
        open: bool,
    },
    /// Open an alternative recorded target for the current exact selection.
    Open(OpenTarget),
    /// Navigate without opening documents, including across page boundaries.
    Move {
        /// Target composition.
        surface: Surface,
        /// Signed row movement.
        delta: i32,
    },
    /// Toggle only a group's disclosure control.
    Toggle {
        /// Target composition.
        surface: Surface,
        /// Stable group identity.
        group: String,
    },
    /// Search the native text index without replacing the graph with results.
    Find {
        /// Target composition.
        surface: Surface,
        /// Literal, case-insensitive text.
        text: String,
    },
    /// Move through global indexed Find matches.
    Match {
        /// Target composition.
        surface: Surface,
        /// Signed match movement.
        delta: i32,
    },
    /// Report only semantic visibility; pixel offsets stay in web-ui.
    Visible {
        /// Target composition.
        surface: Surface,
        /// First visible occurrence.
        anchor: Option<String>,
        /// Group identities currently being read.
        groups: Vec<String>,
        /// Whether new rows may be followed automatically.
        at_newest: bool,
    },
    /// Cancel resumable native construction for this composition.
    Cancel(Surface),
    /// Retire transport work while keeping readable data and interaction state.
    Suspend,
    /// Internal native completion, accepted only through Crux continuations.
    #[serde(skip)]
    #[facet(skip)]
    Completed {
        /// Monotonic request identity.
        request: u64,
        /// Typed platform response.
        result: QueryOutput,
    },
}

pub(in crate::history) fn update(event: Event, model: &mut Model) -> TimelineCommand {
    match event {
        Event::Load(surface) => {
            let state = model.surfaces.entry(surface).or_default();
            if state.active {
                return Command::done();
            }
            state.active = true;
            state.at_newest = true;
            window(model, surface, Position::Latest)
        }
        Event::Seek {
            surface,
            occurrence,
        } => {
            let state = model.surfaces.entry(surface).or_default();
            state.active = true;
            state.view.focus = Some(occurrence.clone());
            state.view.focus_revision = state.view.focus_revision.saturating_add(1);
            state.temporary.clear();
            window(model, surface, Position::Seek(occurrence))
        }
        Event::Reveal { surface, selection } => {
            if selection.occurrence.len() > 256 || !selection.address.is_valid() {
                model.surfaces.entry(surface).or_default().view.state = RequestState::Failed(
                    error("Activity selection requires an exact history destination."),
                );
                return render::render();
            }
            let occurrence = selection.occurrence.clone();
            model.selected = Some(selection);
            let state = model.surfaces.entry(surface).or_default();
            state.view.filter = Filter::default();
            state.view.search = super::model::Search::default();
            update(
                Event::Seek {
                    surface,
                    occurrence,
                },
                model,
            )
        }
        Event::Page { surface, newer } => page(model, surface, newer),
        Event::Refresh(surface) => {
            let state = model.surfaces.entry(surface).or_default();
            if !state.active {
                return Command::done();
            }
            let position = if state.at_newest {
                Position::Latest
            } else {
                Position::Refresh(state.anchor.clone())
            };
            window(model, surface, position)
        }
        Event::Latest(surface) => {
            let state = model.surfaces.entry(surface).or_default();
            state.at_newest = true;
            state.view.new_activity = false;
            state.view.focus = None;
            state.view.focus_revision = state.view.focus_revision.saturating_add(1);
            window(model, surface, Position::Latest)
        }
        Event::Filter { surface, filter } => {
            model.pending.retain(|pending| {
                pending.surface != surface || !matches!(pending.query, QueryAction::Timeline(_))
            });
            let state = model.surfaces.entry(surface).or_default();
            state.view.filter = filter;
            state.temporary.clear();
            state.view.search = super::model::Search::default();
            state.view.focus = None;
            state.view.focus_revision = state.view.focus_revision.saturating_add(1);
            state.at_newest = true;
            state.view.new_activity = false;
            window(model, surface, Position::Latest)
        }
        Event::Select {
            surface,
            occurrence,
            open,
        } => select(model, surface, &occurrence, open),
        Event::Open(target) => {
            let Some(selected) = &model.selected else {
                return Command::done();
            };
            open(model, selected.address.clone(), target, None)
        }
        Event::Move { surface, delta } => move_row(model, surface, delta),
        Event::Toggle { surface, group } => toggle(model, surface, &group),
        Event::Find { surface, text } => find(model, surface, text),
        Event::Match { surface, delta } => navigate_match(model, surface, delta),
        Event::Visible {
            surface,
            anchor,
            groups,
            at_newest,
        } => {
            let state = model.surfaces.entry(surface).or_default();
            state.anchor = anchor;
            state.at_newest = at_newest;
            state.visible = groups.into_iter().take(500).collect();
            state.reading.retain(|group| state.visible.contains(group));
            Command::done()
        }
        Event::Cancel(surface) => {
            model.pending.retain(|pending| {
                pending.surface != surface || !matches!(pending.query, QueryAction::Timeline(_))
            });
            let state = model.surfaces.entry(surface).or_default();
            state.view.state = RequestState::Idle;
            let progress = state.view.progress.take();
            if let Some(progress) = progress {
                request(
                    model,
                    surface,
                    Action::Cancel {
                        build: progress.build,
                    },
                )
            } else {
                render::render()
            }
        }
        Event::Suspend => {
            model.pending.clear();
            for state in model.surfaces.values_mut() {
                state.view.state = RequestState::Idle;
                state.view.progress = None;
            }
            model.open = RequestState::Idle;
            render::render()
        }
        Event::Completed { request, result } => response::complete(model, request, result),
    }
}

pub(super) fn error(message: impl Into<String>) -> EffectError {
    EffectError {
        message: message.into(),
    }
}

pub(super) fn request(model: &mut Model, surface: Surface, action: Action) -> TimelineCommand {
    dispatch(
        model,
        surface,
        QueryAction::Timeline(Box::new(Request {
            version: VERSION,
            action,
        })),
    )
}

fn dispatch(model: &mut Model, surface: Surface, query: QueryAction) -> TimelineCommand {
    let Some(chain) = model.chain.clone() else {
        return Command::done();
    };
    let Ok(id) = model.pending.register(
        Pending {
            surface,
            query: query.clone(),
        },
        false,
    ) else {
        model.surfaces.entry(surface).or_default().view.state =
            RequestState::Failed(error("Activity request identities are exhausted."));
        return render::render();
    };
    Command::request_from_shell(Query {
        chain,
        action: query,
    })
    .then_send(move |result| Event::Completed {
        request: id,
        result,
    })
    .and(render::render())
}

pub(super) fn window(model: &mut Model, surface: Surface, position: Position) -> TimelineCommand {
    model.pending.retain(|pending| pending.surface != surface || !matches!(&pending.query, QueryAction::Timeline(request) if !matches!(request.action, Action::Find { .. })));
    let state = model.surfaces.entry(surface).or_default();
    state.view.state = RequestState::Loading;
    let action = Action::Window {
        view: state.query_view(),
        position,
        limit: if surface == Surface::Mini {
            40
        } else {
            DEFAULT_LIMIT
        },
    };
    state.intent = Some(action.clone());
    request(model, surface, action)
}

fn page(model: &mut Model, surface: Surface, newer: bool) -> TimelineCommand {
    let state = model.surfaces.entry(surface).or_default();
    if state.view.state == RequestState::Loading {
        return Command::done();
    }
    let cursor = state.view.window.as_ref().and_then(|window| {
        if newer {
            window.newer.clone()
        } else {
            window.older.clone()
        }
    });
    cursor.map_or_else(Command::done, |cursor| {
        window(model, surface, Position::Page(cursor))
    })
}

fn select(
    model: &mut Model,
    surface: Surface,
    occurrence: &str,
    activate: bool,
) -> TimelineCommand {
    let Some(row) = model
        .surfaces
        .get(&surface)
        .and_then(|state| state.view.window.as_ref())
        .and_then(|window| window.rows.iter().find(|row| row.occurrence == occurrence))
        .cloned()
    else {
        return Command::done();
    };
    model.selected = Some(Selection {
        occurrence: row.occurrence,
        address: row.address.clone(),
    });
    if activate {
        open(model, row.address, row.open, row.unavailable)
    } else {
        render::render()
    }
}

fn open(
    model: &mut Model,
    address: idle_history::timeline::Target,
    target: OpenTarget,
    notice: Option<String>,
) -> TimelineCommand {
    if !address.is_valid() {
        model.open = RequestState::Failed(error(
            "Native activation requires an exact history destination.",
        ));
        return render::render();
    }
    model
        .pending
        .retain(|pending| !matches!(pending.query, QueryAction::OpenAt { .. }));
    model.open = RequestState::Loading;
    model.notice = notice;
    dispatch(
        model,
        Surface::Editor,
        QueryAction::OpenAt { address, target },
    )
}

pub(super) fn move_row(model: &mut Model, surface: Surface, delta: i32) -> TimelineCommand {
    let state = model.surfaces.entry(surface).or_default();
    let Some(window) = &state.view.window else {
        return Command::done();
    };
    let current = model.selected.as_ref().and_then(|selected| {
        window
            .rows
            .iter()
            .position(|row| row.occurrence == selected.occurrence)
    });
    let next = current.map_or(
        Some(if delta < 0 {
            window.rows.len().saturating_sub(1)
        } else {
            0
        }),
        |current| {
            if delta < 0 {
                current.checked_sub(usize::try_from(delta.unsigned_abs()).unwrap_or(usize::MAX))
            } else {
                current
                    .checked_add(usize::try_from(delta).unwrap_or(usize::MAX))
                    .filter(|index| *index < window.rows.len())
            }
        },
    );
    if let Some(row) = next.and_then(|index| window.rows.get(index)) {
        let id = row.occurrence.clone();
        state.view.focus = Some(id.clone());
        state.view.focus_revision = state.view.focus_revision.saturating_add(1);
        state.pending_move = None;
        select(model, surface, &id, false)
    } else {
        state.pending_move = Some(delta);
        page(model, surface, delta < 0)
    }
}

fn toggle(model: &mut Model, surface: Surface, id: &str) -> TimelineCommand {
    let state = model.surfaces.entry(surface).or_default();
    let group = state
        .view
        .window
        .as_ref()
        .into_iter()
        .flat_map(|window| &window.rows)
        .filter_map(|row| row.group.as_ref())
        .find(|group| group.id == id);
    let Some(group) = group else {
        return Command::done();
    };
    let _previous = state.manual.insert(id.to_owned(), !group.expanded);
    while state.manual.len() > 1_024 {
        let _removed = state.manual.pop_first();
    }
    let _removed = state.temporary.remove(id);
    let position = Position::Refresh(state.anchor.clone());
    window(model, surface, position)
}

fn find(model: &mut Model, surface: Surface, text: String) -> TimelineCommand {
    model.pending.retain(|pending| pending.surface != surface || !matches!(&pending.query, QueryAction::Timeline(request) if matches!(request.action, Action::Find { .. })));
    let state = model.surfaces.entry(surface).or_default();
    let restore = !state.temporary.is_empty();
    state.temporary.clear();
    state.view.search = super::model::Search {
        text: text.clone(),
        ..super::model::Search::default()
    };
    if text.is_empty() {
        let anchor = state.anchor.clone();
        return if restore {
            window(model, surface, Position::Refresh(anchor))
        } else {
            render::render()
        };
    }
    if text.len() > 4_096 {
        state.view.search.text.clear();
        state.view.search.state =
            RequestState::Failed(error("Find accepts up to 4096 bytes of text."));
        return render::render();
    }
    state.view.search.state = RequestState::Loading;
    let action = Action::Find {
        view: state.query_view(),
        text,
        cursor: None,
        limit: DEFAULT_LIMIT,
    };
    request(model, surface, action)
}

pub(super) fn navigate_match(model: &mut Model, surface: Surface, delta: i32) -> TimelineCommand {
    let state = model.surfaces.entry(surface).or_default();
    let search = &mut state.view.search;
    if search.total == 0 {
        return Command::done();
    }
    let amount = u64::from(delta.unsigned_abs())
        .checked_rem(search.total)
        .unwrap_or(0);
    let next = search.current.map_or(
        if delta < 0 {
            search.total.saturating_sub(1)
        } else {
            0
        },
        |position| {
            if delta < 0 {
                position
                    .saturating_add(search.total)
                    .saturating_sub(amount)
                    .checked_rem(search.total)
                    .unwrap_or(0)
            } else {
                position
                    .saturating_add(amount)
                    .checked_rem(search.total)
                    .unwrap_or(0)
            }
        },
    );
    search.current = Some(next);
    if let Some(hit) = next
        .checked_sub(search.offset)
        .and_then(|index| usize::try_from(index).ok())
        .and_then(|index| search.matches.get(index))
        .cloned()
    {
        state.view.focus = Some(hit.occurrence.clone());
        state.view.focus_revision = state.view.focus_revision.saturating_add(1);
        state.temporary.clear();
        if let Some(group) = hit.group {
            let _inserted = state.temporary.insert(group);
        }
        model.selected = Some(Selection {
            occurrence: hit.occurrence.clone(),
            address: hit.address,
        });
        window(model, surface, Position::Seek(hit.occurrence))
    } else if let Some(mut cursor) = search.next.clone() {
        cursor.offset =
            next.saturating_sub(next.checked_rem(u64::from(DEFAULT_LIMIT)).unwrap_or(0));
        search.state = RequestState::Loading;
        let action = Action::Find {
            view: state.query_view(),
            text: state.view.search.text.clone(),
            cursor: Some(cursor),
            limit: DEFAULT_LIMIT,
        };
        request(model, surface, action)
    } else {
        render::render()
    }
}
