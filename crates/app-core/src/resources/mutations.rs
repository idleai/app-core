//! Retry-safe runtime intents and retained progress, separate from discovery.

use crux_core::{Command, render};

use super::{
    Model, ResourceActionStage, ResourceContext, ResourceError, ResourceMutation,
    ResourceMutationView, ResourceOperationKind, ResourceOutput, ResourceProgress,
    ResourceRecoveryAction, ResourceRequest, ResourceResult,
    model::terminal,
    reducer::{ResourceCommand, action_error, dispatch, refresh},
    validation,
};

pub(super) fn execute(
    model: &mut Model,
    request: ResourceRequest,
    mutation: ResourceMutation,
) -> ResourceCommand {
    if let Some(command) = existing_action(model, &request, &mutation) {
        return command;
    }
    if request.request_id.is_empty() || request.expires_at_ms <= model.now_ms {
        return action_error(
            model,
            validation::selection(
                "Resource action needs a persisted retry identity and future first-receipt deadline",
            ),
        );
    }
    if !model.allowed(&mutation) {
        return action_error(
            model,
            validation::selection("Resource action is not currently available for this target"),
        );
    }
    model.mutations.push(ResourceMutationView {
        request: request.clone(),
        mutation: mutation.clone(),
        progress: None,
        pending: true,
        in_flight: true,
        error: None,
        recovery: Vec::new(),
    });
    model.action_error = None;
    dispatch(model, ResourceOperationKind::Mutate { request, mutation })
}

fn existing_action(
    model: &mut Model,
    request: &ResourceRequest,
    mutation: &ResourceMutation,
) -> Option<ResourceCommand> {
    if let Some(old) = model
        .mutations
        .iter()
        .find(|old| old.request.request_id == request.request_id)
    {
        return Some(if &old.request == request && &old.mutation == mutation {
            Command::done()
        } else {
            action_error(
                model,
                validation::selection(
                    "Request identity already belongs to another immutable resource action",
                ),
            )
        });
    }
    None
}

pub(super) fn restore(
    model: &mut Model,
    context: &ResourceContext,
    request: ResourceRequest,
    mutation: ResourceMutation,
) -> ResourceCommand {
    if model.context() != Some(context) {
        return action_error(
            model,
            validation::selection("Persisted resource action belongs to another context"),
        );
    }
    if let Some(command) = existing_action(model, &request, &mutation) {
        return command;
    }
    if request.request_id.is_empty() || request.expires_at_ms == 0 {
        return action_error(
            model,
            validation::selection("Persisted resource action has an invalid identity or deadline"),
        );
    }
    if let Err(error) = validation::mutation(&mutation) {
        return action_error(model, error);
    }
    let id = request.request_id.clone();
    let _inserted = model.status_dirty.insert(id.clone());
    model.mutations.push(ResourceMutationView {
        request,
        mutation,
        progress: None,
        pending: true,
        in_flight: false,
        error: Some(validation::uncertain(
            "Restored action has an unknown outcome; check its original status",
        )),
        recovery: Vec::new(),
    });
    model.action_error = None;
    if model.ready() {
        return recover(model, &id, ResourceRecoveryAction::CheckStatus);
    }
    render::render()
}

pub(super) fn recover(
    model: &mut Model,
    id: &str,
    action: ResourceRecoveryAction,
) -> ResourceCommand {
    let Some(existing) = model
        .mutations
        .iter()
        .find(|mutation| mutation.request.request_id == id)
    else {
        return action_error(
            model,
            validation::selection("No resource action has this request identity"),
        );
    };
    if !model.recovery(existing).contains(&action) {
        return action_error(
            model,
            validation::selection("Resource action does not currently permit this recovery step"),
        );
    }
    let kind = match action {
        ResourceRecoveryAction::CheckStatus => {
            ResourceOperationKind::Status(existing.request.clone())
        }
        ResourceRecoveryAction::Retry => ResourceOperationKind::Mutate {
            request: existing.request.clone(),
            mutation: existing.mutation.clone(),
        },
    };
    if let Some(existing) = model
        .mutations
        .iter_mut()
        .find(|mutation| mutation.request.request_id == id)
    {
        existing.in_flight = true;
        existing.error = None;
    }
    model.action_error = None;
    let _was_dirty = model.status_dirty.remove(id);
    dispatch(model, kind)
}

pub(super) fn invalidate(model: &mut Model) {
    model.status_dirty.extend(
        model
            .mutations
            .iter()
            .filter(|mutation| mutation.pending)
            .map(|mutation| mutation.request.request_id.clone()),
    );
}

pub(super) fn reconcile(model: &mut Model) -> ResourceCommand {
    let pending: Vec<_> = model
        .mutations
        .iter()
        .filter(|mutation| {
            model.status_dirty.contains(&mutation.request.request_id)
                && model
                    .recovery(mutation)
                    .contains(&ResourceRecoveryAction::CheckStatus)
        })
        .map(|mutation| mutation.request.request_id.clone())
        .collect();
    pending.into_iter().fold(Command::done(), |command, id| {
        command.and(recover(model, &id, ResourceRecoveryAction::CheckStatus))
    })
}

pub(super) fn failed(model: &mut Model, kind: &ResourceOperationKind, error: ResourceError) {
    let request = match kind {
        ResourceOperationKind::Mutate { request, .. } | ResourceOperationKind::Status(request) => {
            request
        }
        ResourceOperationKind::Snapshot => return,
    };
    if let Some(existing) = model
        .mutations
        .iter_mut()
        .find(|mutation| &mutation.request == request)
    {
        existing.in_flight = false;
        // Transport/recovery errors do not establish an execution outcome.
        // Only a runtime terminal fact can release conflicting pending work.
        existing.error = Some(error);
    }
}

pub(super) fn complete(
    model: &mut Model,
    kind: &ResourceOperationKind,
    result: ResourceOutput,
) -> ResourceCommand {
    let request = match kind {
        ResourceOperationKind::Mutate { request, .. } | ResourceOperationKind::Status(request) => {
            request
        }
        ResourceOperationKind::Snapshot => return Command::done(),
    };
    let Some(existing) = model
        .mutations
        .iter()
        .find(|mutation| &mutation.request == request)
    else {
        return Command::done();
    };
    let result = result.and_then(|result| match result {
        ResourceResult::Progress(progress) => {
            validate_progress(model, existing, &progress)?;
            Ok(progress)
        }
        ResourceResult::Snapshot(_) => Err(validation::uncertain(
            "Resource action returned discovery instead of its runtime result",
        )),
        ResourceResult::Unknown => Err(validation::uncertain(
            "No retained action result; execution outcome remains unknown",
        )),
    });
    match result {
        Ok(progress) => {
            let succeeded = progress.stage == ResourceActionStage::Succeeded;
            if terminal(&progress.stage) {
                let _was_dirty = model.status_dirty.remove(&request.request_id);
            }
            if let Some(existing) = model
                .mutations
                .iter_mut()
                .find(|mutation| &mutation.request == request)
            {
                existing.pending = !terminal(&progress.stage);
                existing.in_flight = false;
                existing.error = None;
                existing.progress = Some(progress);
            }
            // Even runtime completion is not a replacement of directory health or
            // model choice; fetch current authoritative facts before enabling work.
            if succeeded {
                return refresh(model).and(render::render());
            }
        }
        Err(error) => failed(model, kind, error),
    }
    reconcile(model).and(render::render())
}

fn validate_progress(
    model: &Model,
    existing: &ResourceMutationView,
    value: &ResourceProgress,
) -> Result<(), ResourceError> {
    if model.context() != Some(&value.context)
        || value.request != existing.request
        || value.revision == 0
    {
        return Err(validation::uncertain(
            "Resource action result has a different scope, identity or invalid revision",
        ));
    }
    if let Some(previous) = &existing.progress
        && (value.revision < previous.revision
            || (value.revision == previous.revision && value != previous)
            || (terminal(&previous.stage) && previous.stage != value.stage)
            || (matches!(previous.stage, ResourceActionStage::Running(_))
                && value.stage == ResourceActionStage::Received))
    {
        return Err(validation::uncertain(
            "Resource action result conflicts with its previously confirmed progress",
        ));
    }
    Ok(())
}
