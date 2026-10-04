use crux_core::{App, Command, render};
use serde::{Deserialize, Serialize};

use super::{
    Model, ProjectionFilter, ProjectionKind, ProjectionLoadState, ProjectionOutput,
    ProjectionQuery, ProjectionReference, ProjectionSelection, ViewModel, adapter::error,
};
use crate::{effects::Effect, history, subscriptions::Context};

type ProjectionCommand = Command<Effect, ProjectionEvent>;

/// Projection intents. Only matching host continuations can replace input state.
#[derive(Clone, Debug, Deserialize, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ProjectionEvent {
    /// Bind the current authorized provider/audience and load all destinations.
    Connect(Context),
    /// Revalidate upstream sources and replace all destinations from the start.
    Refresh,
    /// Change the bounded engine read budget (1 through 1000) and refresh.
    SetLimit(u32),
    /// Filter one destination using supplied fields only.
    SetFilter {
        /// Destination whose local filter changes.
        kind: ProjectionKind,
        /// New conjunctive filter.
        filter: ProjectionFilter,
    },
    /// Select a stable row key, or clear selection.
    Select(Option<ProjectionSelection>),
    /// Open a reference actually supplied on this row in shared history state.
    Inspect {
        /// Row that supplied the reference.
        selection: ProjectionSelection,
        /// Exact source or related address; prefixes and foreign references fail.
        reference: ProjectionReference,
    },
    /// Retire pending reads after delivery continuity is lost.
    Suspend,
    /// Resume with an atomic replacement; never resume by maximum operation ID.
    Reconnect,
    /// Clear this client's projection state and retire all continuations.
    Disconnect,
    /// Reconcile a background change, allowing hosts to reuse recently read sources.
    Changed,
    /// Internal result, excluded from serialized client actions.
    #[serde(skip)]
    #[facet(skip)]
    Completed {
        /// Non-reusable client-local continuation identity.
        token: u64,
        /// Result returned for the corresponding host query.
        result: ProjectionOutput,
    },
}

/// Shared projection reducer; no controller inference or persistence.
#[derive(Debug, Default)]
pub struct Projections;

impl App for Projections {
    type Event = ProjectionEvent;
    type Model = Model;
    type ViewModel = ViewModel;
    type Effect = Effect;

    fn update(&self, event: ProjectionEvent, model: &mut Model) -> ProjectionCommand {
        match event {
            ProjectionEvent::Connect(context) => {
                if [
                    &context.provider,
                    &context.workspace,
                    &context.contributor,
                    &context.chain,
                ]
                .iter()
                .any(|part| part.is_empty())
                {
                    return action_error(
                        model,
                        "Projection context requires provider, workspace, contributor and chain",
                    );
                }
                if model.context() == Some(&context) {
                    return Command::done();
                }
                model.reset();
                model.context = Some(context);
                refresh(model, false)
            }
            ProjectionEvent::Refresh => refresh(model, true),
            ProjectionEvent::Changed => refresh(model, false),
            ProjectionEvent::SetLimit(limit) => {
                if !(1..=1000).contains(&limit) {
                    return action_error(model, "Projection read limit must be between 1 and 1000");
                }
                if model.limit.unwrap_or(100) != limit {
                    model.retire_reads();
                }
                model.limit = Some(limit);
                refresh(model, false)
            }
            ProjectionEvent::SetFilter { kind, filter } => {
                drop(model.filters.insert(kind, filter));
                model.action_error = None;
                render::render()
            }
            ProjectionEvent::Select(selection) => {
                if selection
                    .as_ref()
                    .is_some_and(|selected| model.row(selected).is_none())
                {
                    return action_error(model, "Projection row is not in the current input");
                }
                model.selected = selection;
                model.action_error = None;
                render::render()
            }
            ProjectionEvent::Inspect {
                selection,
                reference,
            } => inspect(model, &selection, reference),
            ProjectionEvent::Suspend => {
                if model.context.is_none() {
                    return Command::done();
                }
                model.retire_reads();
                model.suspended = true;
                model.stale = true;
                model.load = ProjectionLoadState::Suspended;
                render::render()
            }
            ProjectionEvent::Reconnect => {
                model.retire_reads();
                model.suspended = false;
                refresh(model, false)
            }
            ProjectionEvent::Disconnect => {
                model.reset();
                render::render()
            }
            ProjectionEvent::Completed { token, result } => complete(model, token, result),
        }
    }

    fn view(&self, model: &Model) -> ViewModel {
        model.view()
    }
}

fn action_error(model: &mut Model, message: &str) -> ProjectionCommand {
    model.action_error = Some(error(message));
    render::render()
}

fn refresh(model: &mut Model, refresh_sources: bool) -> ProjectionCommand {
    let Some(context) = model.context.clone().filter(|_| !model.suspended) else {
        return Command::done();
    };
    if !model.requests.is_empty() {
        model.refresh_again = Some(model.refresh_again.unwrap_or(false) || refresh_sources);
        return Command::done();
    }
    model.stale = true;
    let query = ProjectionQuery {
        context,
        limit: model.limit.unwrap_or(100),
        refresh_sources,
    };
    let Ok(token) = model.requests.register(query.clone(), false) else {
        model.load = ProjectionLoadState::Failed(error("Projection request identities exhausted"));
        return render::render();
    };
    model.load = ProjectionLoadState::Loading;
    model.action_error = None;
    Command::request_from_shell(query)
        .then_send(move |result| ProjectionEvent::Completed { token, result })
        .and(render::render())
}

fn complete(model: &mut Model, token: u64, result: ProjectionOutput) -> ProjectionCommand {
    let Some((query, _window)) = model.requests.take(token) else {
        return Command::done();
    };
    if model.context() != Some(&query.context) {
        return Command::done();
    }
    let result = result.and_then(|snapshot| {
        if snapshot.workspace_id != query.context.workspace || snapshot.chain != query.context.chain
        {
            return Err(error(
                "Projection input belongs to a different workspace or chain",
            ));
        }
        let shared = idle_protocol::v1::projections::ProjectionSnapshot::try_from(snapshot)?;
        shared.try_into()
    });
    match result {
        Ok(snapshot) => {
            model.snapshot = Some(snapshot);
            if model
                .selected
                .as_ref()
                .is_some_and(|selected| model.row(selected).is_none())
            {
                model.selected = None;
            }
            model.load = ProjectionLoadState::Ready;
            model.stale = false;
        }
        Err(failure) => {
            model.load = ProjectionLoadState::Failed(failure);
            model.stale = true;
        }
    }
    if let Some(refresh_sources) = model.refresh_again.take() {
        return refresh(model, refresh_sources);
    }
    render::render()
}

fn inspect(
    model: &mut Model,
    selection: &ProjectionSelection,
    reference: ProjectionReference,
) -> ProjectionCommand {
    if !model
        .row(selection)
        .is_some_and(|row| row.sources.contains(&reference) || row.related.contains(&reference))
    {
        return action_error(
            model,
            "History reference is not supplied on the selected projection row",
        );
    }
    model.history = Some(history::Event::Select(history::Selected {
        item: reference.item,
        observation: reference.observation,
    }));
    model.action_error = None;
    render::render()
}
