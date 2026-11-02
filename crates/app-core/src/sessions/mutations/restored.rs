use super::{
    Command, Model, SessionAction, SessionCommand, SessionError, SessionErrorCode, SessionInputRef,
    SessionMutation, SessionMutationState, SessionMutationView, SessionOperation,
    SessionPromptView, State, action_error, recover, validation,
};

pub(in crate::sessions) fn restore(
    model: &mut Model,
    operation: SessionOperation,
) -> SessionCommand {
    let checked = validate(&model.state, &operation);
    if let Err(error) = checked {
        return action_error(model, error);
    }
    let SessionAction::Mutate {
        request: attribution,
        mutation,
    } = operation.action
    else {
        return Command::done();
    };
    if let Some(previous) = model
        .state
        .mutations
        .iter()
        .find(|previous| previous.request.key() == attribution.key())
    {
        if previous.request != attribution || previous.mutation != mutation {
            return action_error(
                model,
                validation::error(
                    SessionErrorCode::IdempotencyConflict,
                    "Saved request identity belongs to different session intent",
                ),
            );
        }
        return recover(model, &attribution.mutation.request_id);
    }
    if let SessionMutation::Submit { session_id, text } = &mutation {
        if let Some(prompt) = model
            .state
            .prompts
            .iter_mut()
            .find(|prompt| prompt.input.request == attribution.key())
        {
            if prompt.input.session_id != *session_id
                || prompt.contributor != attribution.contributor
                || prompt.text.as_ref().is_some_and(|known| known != text)
            {
                return action_error(
                    model,
                    validation::invalid("Saved prompt conflicts with the runtime's retained input"),
                );
            }
            prompt.text = Some(text.clone());
        } else {
            model.state.prompts.push(SessionPromptView {
                input: SessionInputRef {
                    session_id: session_id.clone(),
                    request: attribution.key(),
                },
                contributor: attribution.contributor.clone(),
                text: Some(text.clone()),
                submission: None,
                runtime: None,
            });
        }
    }
    let id = attribution.mutation.request_id.clone();
    model.state.mutations.push(SessionMutationView {
        request: attribution,
        mutation,
        state: SessionMutationState::Unknown,
    });
    recover(model, &id)
}

fn validate(state: &State, operation: &SessionOperation) -> Result<(), SessionError> {
    let snapshot = state
        .snapshot
        .as_ref()
        .filter(|_| state.ready())
        .ok_or_else(|| validation::invalid("Load session state before restoring saved requests"))?;
    let SessionAction::Mutate { request, mutation } = &operation.action else {
        return Err(validation::invalid(
            "Only saved outgoing mutations can be restored",
        ));
    };
    if operation.context != snapshot.context
        || request.workspace_id != snapshot.context.workspace_id
        || request.contributor != snapshot.contributor
    {
        return Err(validation::error(
            SessionErrorCode::Forbidden,
            "Saved request belongs to another session connection",
        ));
    }
    validation::nonempty(&request.mutation.request_id)?;
    if mutation
        .session_id()
        .is_some_and(|id| !state.can_observe(id))
    {
        return Err(validation::error(
            SessionErrorCode::Forbidden,
            "Saved request is outside current session access",
        ));
    }
    Ok(())
}
