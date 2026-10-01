//! Workspace transitions and context-correlated host continuations.

use crux_core::{App, Command, render};
use serde::{Deserialize, Serialize};

use super::{
    Model, NavigationSection, PresenceSnapshot, WorkspaceError, WorkspaceErrorKind, WorkspaceInfo,
    WorkspaceMode, WorkspaceOperation, WorkspaceOutput, WorkspaceRequestState, WorkspaceResult,
    WorkspaceSnapshot, WorkspaceViewModel, validation,
};
use crate::effects::Effect;

type WorkspaceCommand = Command<Effect, WorkspaceEvent>;

/// Shared client navigation actions. Host results enter through continuations only.
#[derive(Clone, Debug, Deserialize, Serialize, facet::Facet)]
#[repr(u8)]
pub enum WorkspaceEvent {
    /// Discover or refresh available workspaces through configured host adapters.
    Load,
    /// Select a visible workspace, loading members and connecting its chain.
    SelectWorkspace(String),
    /// Select an attached repository, or the whole managed workspace with `None`.
    SelectRepository(Option<String>),
    /// Select a shared navigation destination in the current workspace.
    Navigate(NavigationSection),
    /// Refresh or retry selected workspace metadata and membership.
    RefreshWorkspace,
    /// Refresh or retry current member presence independently of metadata.
    RefreshPresence,
    /// Advance the host's Unix clock for presence expiry; older ticks are ignored.
    Tick(u64),
    /// Clear navigation and invalidate pending results, without deleting resources.
    Disconnect,
    /// Internal one-shot completion; serialized actions cannot forge provider data.
    #[serde(skip)]
    #[facet(skip)]
    Completed {
        /// Monotonic client-local continuation token.
        request: u64,
        /// Typed provider result.
        result: WorkspaceOutput,
    },
}

/// Workspace reducer shared by standalone and managed clients.
#[derive(Debug, Default)]
pub struct Workspace;

impl App for Workspace {
    type Event = WorkspaceEvent;
    type Model = Model;
    type ViewModel = WorkspaceViewModel;
    type Effect = Effect;

    fn update(&self, event: WorkspaceEvent, model: &mut Model) -> WorkspaceCommand {
        match event {
            WorkspaceEvent::Load => {
                model.connected = true;
                request(model, WorkspaceOperation::List)
            }
            WorkspaceEvent::SelectWorkspace(id) => select_workspace(model, &id),
            WorkspaceEvent::SelectRepository(id) => select_repository(model, id),
            WorkspaceEvent::Navigate(section) => {
                if model.selected().is_none() {
                    return selection_error(model, "Select a workspace before navigating");
                }
                model.view.section = section;
                model.view.selection_error = None;
                render::render()
            }
            WorkspaceEvent::RefreshWorkspace => refresh_snapshot(model),
            WorkspaceEvent::RefreshPresence => refresh_presence(model),
            WorkspaceEvent::Tick(now_ms) => {
                if now_ms <= model.now_ms {
                    return Command::done();
                }
                model.now_ms = now_ms;
                model.presence.retain(|entry| entry.valid_until_ms > now_ms);
                render::render()
            }
            WorkspaceEvent::Disconnect => {
                clear_selection(model);
                model.view = WorkspaceViewModel::default();
                model.pending.clear();
                model.connected = false;
                render::render()
            }
            WorkspaceEvent::Completed { request, result } => complete(model, request, result),
        }
    }

    fn view(&self, model: &Model) -> WorkspaceViewModel {
        model.render()
    }
}

fn selection_error(model: &mut Model, message: &str) -> WorkspaceCommand {
    model.view.selection_error = Some(WorkspaceError {
        kind: WorkspaceErrorKind::InvalidSelection,
        message: message.into(),
    });
    render::render()
}

fn clear_presence(model: &mut Model) {
    model
        .pending
        .retain(|_, operation| !matches!(operation, WorkspaceOperation::Presence { .. }));
    model.presence.clear();
    model.view.presence_state = WorkspaceRequestState::Idle;
}

fn clear_selection(model: &mut Model) {
    model
        .pending
        .retain(|_, operation| matches!(operation, WorkspaceOperation::List));
    model.presence.clear();
    model.view.selected_workspace = None;
    model.view.selected_repository = None;
    model.view.snapshot = None;
    model.view.snapshot_state = WorkspaceRequestState::Idle;
    model.view.presence_state = WorkspaceRequestState::Idle;
    model.view.section = NavigationSection::Workspace;
    model.view.selection_error = None;
}

fn select_workspace(model: &mut Model, id: &str) -> WorkspaceCommand {
    if !model.view.workspaces.iter().any(|info| info.id == id) {
        return selection_error(model, "Workspace is not in the authorized directory");
    }
    if model.view.selected_workspace.as_deref() == Some(id) {
        model.view.selection_error = None;
        return render::render();
    }
    clear_selection(model);
    model.connected = true;
    model.view.selected_workspace = Some(id.into());
    reconcile_repository(model, true);
    refresh_snapshot(model)
}

fn select_repository(model: &mut Model, id: Option<String>) -> WorkspaceCommand {
    let Some(info) = model.selected() else {
        return selection_error(model, "Select a workspace before selecting a repository");
    };
    let valid = match &id {
        Some(id) => info
            .repositories
            .iter()
            .any(|repository| &repository.id == id),
        None => info.mode == WorkspaceMode::Managed,
    };
    if !valid {
        return selection_error(
            model,
            "Repository is not attached to the selected workspace",
        );
    }
    model.view.selected_repository = id;
    model.view.selection_error = None;
    render::render()
}

fn reconcile_repository(model: &mut Model, initial: bool) {
    let Some(info) = model.selected() else { return };
    let current = model
        .view
        .selected_repository
        .as_ref()
        .filter(|id| {
            info.repositories
                .iter()
                .any(|repository| &repository.id == *id)
        })
        .cloned();
    model.view.selected_repository =
        if info.mode == WorkspaceMode::Standalone || (initial && info.repositories.len() == 1) {
            info.repositories
                .first()
                .map(|repository| repository.id.clone())
        } else {
            current
        };
}

fn refresh_snapshot(model: &mut Model) -> WorkspaceCommand {
    let Some(info) = model.selected() else {
        return Command::done();
    };
    let operation = WorkspaceOperation::Snapshot {
        workspace_id: info.id.clone(),
        mode: info.mode,
    };
    clear_presence(model);
    request(model, operation)
}

fn refresh_presence(model: &mut Model) -> WorkspaceCommand {
    if model.view.snapshot_state != WorkspaceRequestState::Ready {
        return Command::done();
    }
    let Some(snapshot) = &model.view.snapshot else {
        return Command::done();
    };
    request(
        model,
        WorkspaceOperation::Presence {
            workspace_id: snapshot.workspace.id.clone(),
            mode: snapshot.workspace.mode,
        },
    )
}

fn state<'a>(
    model: &'a mut Model,
    operation: &WorkspaceOperation,
) -> &'a mut WorkspaceRequestState {
    match operation {
        WorkspaceOperation::List => &mut model.view.directory_state,
        WorkspaceOperation::Snapshot { .. } => &mut model.view.snapshot_state,
        WorkspaceOperation::Presence { .. } => &mut model.view.presence_state,
    }
}

fn request(model: &mut Model, operation: WorkspaceOperation) -> WorkspaceCommand {
    if model.pending.values().any(|pending| *pending == operation) {
        return Command::done();
    }
    let Some(token) = model.next_request.checked_add(1) else {
        return fail(
            model,
            &operation,
            validation::invalid("Workspace request identity exhausted"),
        );
    };
    model.next_request = token;
    *state(model, &operation) = WorkspaceRequestState::Loading;
    let _previous = model.pending.insert(token, operation.clone());
    Command::request_from_shell(operation)
        .then_send(move |result| WorkspaceEvent::Completed {
            request: token,
            result,
        })
        .and(render::render())
}

fn complete(model: &mut Model, token: u64, result: WorkspaceOutput) -> WorkspaceCommand {
    let Some(operation) = model.pending.remove(&token) else {
        return Command::done();
    };
    let result = match (result, &operation) {
        (Ok(WorkspaceResult::Directory(workspaces)), WorkspaceOperation::List) => {
            accept_list(model, workspaces)
        }
        (
            Ok(WorkspaceResult::Snapshot(snapshot)),
            WorkspaceOperation::Snapshot { workspace_id, .. },
        ) => accept_snapshot(model, workspace_id, snapshot),
        (
            Ok(WorkspaceResult::Presence(presence)),
            WorkspaceOperation::Presence { workspace_id, .. },
        ) => accept_presence(model, workspace_id, presence),
        (Err(error), _) => Err(error),
        (
            Ok(
                WorkspaceResult::Directory(_)
                | WorkspaceResult::Snapshot(_)
                | WorkspaceResult::Presence(_),
            ),
            _,
        ) => Err(validation::invalid(
            "Workspace response does not match the pending operation",
        )),
    };
    result.unwrap_or_else(|error| fail(model, &operation, error))
}

fn accept_list(
    model: &mut Model,
    workspaces: Vec<WorkspaceInfo>,
) -> Result<WorkspaceCommand, WorkspaceError> {
    validation::unique_ids(workspaces.iter().map(|info| info.id.as_str()))?;
    // Validate atomically; one malformed row cannot change any accepted binding.
    let mut known = model.known.clone();
    for info in &workspaces {
        validation::remember(&mut known, info)?;
    }
    let previous = model.selected().cloned();
    model.known = known;
    model.view.workspaces = workspaces;
    model.view.directory_state = WorkspaceRequestState::Ready;
    let current = model.selected().cloned();
    if current.is_none() {
        clear_selection(model);
    } else if previous != current {
        reconcile_repository(model, false);
        model
            .pending
            .retain(|_, operation| !matches!(operation, WorkspaceOperation::Snapshot { .. }));
        return Ok(refresh_snapshot(model));
    }
    Ok(render::render())
}

fn accept_snapshot(
    model: &mut Model,
    workspace_id: &str,
    snapshot: WorkspaceSnapshot,
) -> Result<WorkspaceCommand, WorkspaceError> {
    if snapshot.workspace.id != workspace_id
        || model.view.selected_workspace.as_deref() != Some(workspace_id)
    {
        return Err(validation::invalid(
            "Snapshot belongs to a different workspace",
        ));
    }
    validation::snapshot(&snapshot)?;
    if let Some(previous) = &model.view.snapshot {
        validation::member_revisions(previous, &snapshot)?;
    }
    validation::remember(&mut model.known, &snapshot.workspace)?;
    if let Some(info) = model
        .view
        .workspaces
        .iter_mut()
        .find(|info| info.id == workspace_id)
    {
        *info = snapshot.workspace.clone();
    }
    model.view.snapshot = Some(snapshot);
    model.view.snapshot_state = WorkspaceRequestState::Ready;
    model.view.selection_error = None;
    reconcile_repository(model, false);
    clear_presence(model);
    Ok(refresh_presence(model))
}

fn accept_presence(
    model: &mut Model,
    workspace_id: &str,
    presence: PresenceSnapshot,
) -> Result<WorkspaceCommand, WorkspaceError> {
    if presence.workspace_id != workspace_id
        || model.view.selected_workspace.as_deref() != Some(workspace_id)
    {
        return Err(validation::invalid(
            "Presence belongs to a different workspace",
        ));
    }
    let snapshot = model
        .view
        .snapshot
        .as_ref()
        .ok_or_else(|| validation::invalid("Presence requires workspace metadata"))?;
    validation::presence(&presence, snapshot)?;
    model.now_ms = model.now_ms.max(presence.as_of_ms);
    model.presence = presence.entries;
    model.view.presence_state = WorkspaceRequestState::Ready;
    Ok(render::render())
}

fn fail(
    model: &mut Model,
    operation: &WorkspaceOperation,
    error: WorkspaceError,
) -> WorkspaceCommand {
    if matches!(
        error.kind,
        WorkspaceErrorKind::Unauthenticated
            | WorkspaceErrorKind::Forbidden
            | WorkspaceErrorKind::NotFound
    ) {
        match operation {
            WorkspaceOperation::List => {
                model.view.workspaces.clear();
                clear_selection(model);
            }
            WorkspaceOperation::Snapshot { workspace_id, .. }
            | WorkspaceOperation::Presence { workspace_id, .. } => {
                model
                    .view
                    .workspaces
                    .retain(|info| info.id != *workspace_id);
                clear_selection(model);
                // A directory request started before revocation cannot resurrect access.
                model.pending.clear();
                if model.view.directory_state == WorkspaceRequestState::Loading {
                    model.view.directory_state = WorkspaceRequestState::Failed(error.clone());
                }
            }
        }
    }
    if matches!(operation, WorkspaceOperation::Presence { .. }) {
        model.presence.clear();
    }
    *state(model, operation) = WorkspaceRequestState::Failed(error);
    render::render()
}
