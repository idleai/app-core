//! Retry-safe creation, sharing and attributed prompt dispatch.

use crux_core::{Command, render};

use super::{
    Model, SessionAcknowledgement, SessionAction, SessionCapability, SessionError,
    SessionErrorCode, SessionGrantStatus, SessionInputRef, SessionKind, SessionMutation,
    SessionMutationId, SessionMutationState, SessionMutationView, SessionOperation,
    SessionPermission, SessionPromptView, SessionRequest, SessionResult, SessionRetryAdvice,
    model::State,
    reducer::{SessionCommand, action_error, no_selection, request},
    validation,
};

fn authorize(state: &State, mutation: &SessionMutation, retry: bool) -> Result<(), SessionError> {
    if !state.ready() {
        return Err(validation::error(
            SessionErrorCode::Unavailable,
            "Refresh session state before performing this action",
        ));
    }
    let snapshot = state
        .snapshot
        .as_ref()
        .ok_or_else(|| validation::invalid("Session actions require a snapshot"))?;
    if !state.member_active(&snapshot.context.contributor_id) {
        return Err(validation::error(
            SessionErrorCode::Forbidden,
            "Workspace membership is not active",
        ));
    }
    match mutation {
        SessionMutation::Create(draft) => {
            if snapshot.capabilities.create != SessionCapability::Available {
                return Err(validation::error(
                    SessionErrorCode::UnsupportedOperation,
                    "Runtime session creation is unavailable",
                ));
            }
            validation::nonempty(&draft.title)?;
            validation::nonempty(&draft.host_id)?;
            if draft.parent.as_ref().is_some_and(|id| !state.visible(id)) {
                return Err(validation::error(
                    SessionErrorCode::InvalidSelection,
                    "Parent session is not visible",
                ));
            }
        }
        SessionMutation::Submit { session_id, text } => {
            validation::nonempty(text)?;
            permitted(state, session_id, SessionPermission::SubmitInput)?;
        }
        SessionMutation::Invite {
            session_id,
            grant_id,
            grantee,
            permissions,
            expires_at_ms,
        } => {
            permitted(state, session_id, SessionPermission::Invite)?;
            validation::nonempty(grant_id)?;
            if !state.member_active(grantee) {
                return Err(validation::error(
                    SessionErrorCode::Forbidden,
                    "Invite an active workspace contributor",
                ));
            }
            if !retry && state.known_grants.contains_key(grant_id) {
                return Err(validation::error(
                    SessionErrorCode::Conflict,
                    "Grant identities cannot be reused",
                ));
            }
            if permissions.is_empty()
                || permissions.iter().enumerate().any(|(index, permission)| {
                    permissions
                        .iter()
                        .take(index)
                        .any(|other| other == permission)
                })
            {
                return Err(validation::invalid(
                    "An invitation requires explicit unique session permissions",
                ));
            }
            if !retry && expires_at_ms.is_some_and(|expiry| expiry <= state.now_ms) {
                return Err(validation::invalid(
                    "Invitation expiry must be in the future",
                ));
            }
        }
        SessionMutation::Revoke {
            session_id,
            grant_id,
            expected_revision,
        } => {
            permitted(state, session_id, SessionPermission::Invite)?;
            if !retry
                && !snapshot.grants.iter().any(|grant| {
                    grant.id == *grant_id
                        && grant.session_id == *session_id
                        && grant.revision == *expected_revision
                        && grant.status == SessionGrantStatus::Active
                })
            {
                return Err(validation::error(
                    SessionErrorCode::StaleRevision,
                    "Revocation requires the current active session grant",
                ));
            }
        }
    }
    Ok(())
}

fn permitted(state: &State, id: &str, permission: SessionPermission) -> Result<(), SessionError> {
    let session = state.session(id).ok_or_else(|| {
        validation::error(
            SessionErrorCode::InvalidSelection,
            "Session is absent from the current directory",
        )
    })?;
    if state.actions(session).contains(&permission) {
        Ok(())
    } else {
        Err(validation::error(
            SessionErrorCode::Forbidden,
            "Session permission or connected adapter is unavailable",
        ))
    }
}

pub(super) fn start(
    model: &mut Model,
    id: SessionMutationId,
    mutation: SessionMutation,
) -> SessionCommand {
    let result = prepare(&model.state, id, mutation);
    let (attribution, mutation) = match result {
        Ok(Some(value)) => value,
        Ok(None) => return Command::done(),
        Err(error) => return action_error(model, error),
    };
    if let SessionMutation::Submit { session_id, text } = &mutation {
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
    model.state.mutations.push(SessionMutationView {
        request: attribution.clone(),
        mutation: mutation.clone(),
        state: SessionMutationState::Pending,
    });
    model.state.action_error = None;
    request(
        model,
        SessionAction::Mutate {
            request: attribution,
            mutation,
        },
    )
}

fn prepare(
    state: &State,
    id: SessionMutationId,
    mutation: SessionMutation,
) -> Result<Option<(SessionRequest, SessionMutation)>, SessionError> {
    validation::nonempty(&id.request_id)?;
    let snapshot = state.snapshot.as_ref().ok_or_else(|| {
        validation::error(
            SessionErrorCode::Unavailable,
            "Connect a session adapter before performing this action",
        )
    })?;
    let request = SessionRequest {
        workspace_id: snapshot.context.workspace_id.clone(),
        contributor: snapshot.contributor.clone(),
        mutation: id,
    };
    if let Some(old) = state
        .mutations
        .iter()
        .find(|old| old.request.key() == request.key())
    {
        return if old.request == request && old.mutation == mutation {
            Ok(None)
        } else {
            Err(validation::error(
                SessionErrorCode::IdempotencyConflict,
                "Request identity was already used with different session intent",
            ))
        };
    }
    if state
        .prompts
        .iter()
        .any(|prompt| prompt.input.request == request.key())
    {
        return Err(validation::error(
            SessionErrorCode::IdempotencyConflict,
            "Recovered input already uses this request identity",
        ));
    }
    if request.mutation.expires_at_ms <= state.now_ms {
        return Err(validation::error(
            SessionErrorCode::RequestExpired,
            "First-receipt deadline has elapsed",
        ));
    }
    authorize(state, &mutation, false)?;
    Ok(Some((request, mutation)))
}

pub(super) fn revoke(model: &mut Model, id: SessionMutationId, grant_id: String) -> SessionCommand {
    let Some(session_id) = model.state.selected.clone() else {
        return no_selection(model);
    };
    let grant = model.state.snapshot.as_ref().and_then(|snapshot| {
        snapshot
            .grants
            .iter()
            .find(|grant| grant.id == grant_id && grant.session_id == session_id)
    });
    let Some(grant) = grant else {
        return action_error(
            model,
            validation::error(
                SessionErrorCode::InvalidSelection,
                "Grant does not belong to the selected session",
            ),
        );
    };
    let expected_revision = grant.revision;
    start(
        model,
        id,
        SessionMutation::Revoke {
            session_id,
            grant_id,
            expected_revision,
        },
    )
}

pub(super) fn retry(model: &mut Model, id: &str) -> SessionCommand {
    let Some(old) = model
        .state
        .mutations
        .iter()
        .find(|mutation| mutation.request.mutation.request_id == id)
        .cloned()
    else {
        return action_error(
            model,
            validation::error(
                SessionErrorCode::InvalidSelection,
                "Original mutation is not available for retry",
            ),
        );
    };
    if model.requests.values().any(|operation| matches!(&operation.action, SessionAction::Mutate { request, .. } if *request == old.request)) {
        return Command::done();
    }
    let allowed = matches!(&old.state, SessionMutationState::Failed(SessionError {
        retry: SessionRetryAdvice::SameRequest { not_before_ms }, ..
    }) if not_before_ms.is_none_or(|time| model.state.now_ms >= time));
    if !allowed
        || model
            .state
            .prompts
            .iter()
            .any(|prompt| prompt.input.request == old.request.key() && prompt.runtime.is_some())
    {
        return action_error(
            model,
            validation::error(
                SessionErrorCode::Conflict,
                "Retry advice or confirmed runtime state requires reconciliation instead",
            ),
        );
    }
    // Recheck present access, but keep the original deadline and identity unchanged.
    if let Err(error) = authorize(&model.state, &old.mutation, true) {
        return action_error(model, error);
    }
    if let Some(mutation) = model
        .state
        .mutations
        .iter_mut()
        .find(|mutation| mutation.request == old.request)
    {
        mutation.state = SessionMutationState::Pending;
    }
    model.state.action_error = None;
    request(
        model,
        SessionAction::Mutate {
            request: old.request,
            mutation: old.mutation,
        },
    )
}

pub(super) fn recover(model: &mut Model, id: &str) -> SessionCommand {
    let Some(mutation) = model
        .state
        .mutations
        .iter()
        .find(|mutation| mutation.request.mutation.request_id == id)
    else {
        return action_error(
            model,
            validation::error(
                SessionErrorCode::InvalidSelection,
                "Original mutation is not available for status lookup",
            ),
        );
    };
    if mutation
        .mutation
        .session_id()
        .is_some_and(|id| !model.state.visible(id))
    {
        return action_error(
            model,
            validation::error(
                SessionErrorCode::Forbidden,
                "Session participation is no longer active",
            ),
        );
    }
    request(model, SessionAction::RequestStatus(mutation.request.key()))
}

pub(super) fn complete(
    model: &mut Model,
    operation: &SessionOperation,
    result: SessionResult,
) -> Result<SessionCommand, SessionError> {
    match (&operation.action, result) {
        (
            SessionAction::Mutate {
                request,
                mutation: SessionMutation::Submit { session_id, .. },
            },
            SessionResult::Input(update),
        ) if update.input.request == request.key() && update.input.session_id == *session_id => {
            validation::input(&mut model.state, update, validation::InputSource::Runtime)?;
            if let Some(mutation) = model
                .state
                .mutations
                .iter_mut()
                .find(|mutation| mutation.request == *request)
            {
                mutation.state = SessionMutationState::RuntimeReported;
            }
        }
        (SessionAction::InputStatus(input), SessionResult::Input(update))
            if *input == update.input =>
        {
            validation::input(&mut model.state, update, validation::InputSource::Runtime)?;
        }
        (
            SessionAction::Mutate {
                request,
                mutation: SessionMutation::Create(draft),
            },
            SessionResult::Created {
                request: key,
                session,
            },
        ) => {
            if key != request.key()
                || session.owner != request.contributor.contributor_id
                || session.kind != SessionKind::Runner
                || session.title != draft.title
                || session.runtime.host_id != draft.host_id
                || session.parent != draft.parent
            {
                return Err(validation::invalid(
                    "Runtime creation result does not match the original create request",
                ));
            }
            let mut bindings = model.bindings.clone();
            validation::remember(&mut bindings, &operation.context, &session)?;
            let publish = match model.state.known_sessions.get(&session.id) {
                Some(known) if known.revision >= session.revision => {
                    validation::session_revision(&session, known)?;
                    false
                }
                Some(known) => {
                    validation::session_revision(known, &session)?;
                    true
                }
                None => true,
            };
            let snapshot =
                model.state.snapshot.as_mut().ok_or_else(|| {
                    validation::invalid("Creation result has no current directory")
                })?;
            if publish {
                if let Some(old) = snapshot
                    .sessions
                    .iter_mut()
                    .find(|old| old.id == session.id)
                {
                    validation::session_revision(old, &session)?;
                    *old = session.clone();
                } else {
                    snapshot.sessions.push(session.clone());
                }
            }
            let original = model
                .state
                .mutations
                .iter_mut()
                .find(|old| old.request == *request)
                .ok_or_else(|| validation::invalid("Creation result has no original request"))?;
            original.state = SessionMutationState::Created(session.clone());
            if publish {
                drop(
                    model
                        .state
                        .known_sessions
                        .insert(session.id.clone(), session),
                );
            }
            model.bindings = bindings;
        }
        (SessionAction::Mutate { request, mutation }, SessionResult::Acknowledged(ack)) => {
            acknowledge(&mut model.state, request, mutation, ack, false)?;
        }
        (SessionAction::RequestStatus(key), SessionResult::Acknowledged(ack)) => {
            let old = model
                .state
                .mutations
                .iter()
                .find(|mutation| mutation.request.key() == *key)
                .cloned()
                .ok_or_else(|| validation::invalid("Recovered receipt has no original request"))?;
            acknowledge(&mut model.state, &old.request, &old.mutation, ack, true)?;
        }
        (SessionAction::RequestStatus(key), SessionResult::Unknown) => {
            if let Some(mutation) = model
                .state
                .mutations
                .iter_mut()
                .find(|mutation| mutation.request.key() == *key)
                && !matches!(
                    mutation.state,
                    SessionMutationState::Acknowledged(_)
                        | SessionMutationState::Created(_)
                        | SessionMutationState::RuntimeReported
                )
            {
                mutation.state = SessionMutationState::Unknown;
            }
        }
        _ => {
            return Err(validation::invalid(
                "Session response does not match its pending mutation or lookup",
            ));
        }
    }
    Ok(render::render())
}

fn acknowledge(
    state: &mut State,
    request: &SessionRequest,
    mutation: &SessionMutation,
    ack: SessionAcknowledgement,
    from_status: bool,
) -> Result<(), SessionError> {
    let receipt = match &ack {
        SessionAcknowledgement::Received(receipt)
            if matches!(mutation, SessionMutation::Submit { .. }) =>
        {
            receipt
        }
        SessionAcknowledgement::Committed {
            receipt,
            through,
            committed_at_ms,
        } if matches!(
            mutation,
            SessionMutation::Invite { .. } | SessionMutation::Revoke { .. }
        ) || (from_status && matches!(mutation, SessionMutation::Create(_))) =>
        {
            let context = state
                .context
                .as_ref()
                .ok_or_else(|| validation::invalid("Commit requires a session context"))?;
            validation::cursor(through, context)?;
            if *committed_at_ms < receipt.received_at_ms {
                return Err(validation::invalid("Commit precedes its original receipt"));
            }
            receipt
        }
        SessionAcknowledgement::Received(_) | SessionAcknowledgement::Committed { .. } => {
            return Err(validation::invalid(
                "Acknowledgement stage does not match the session mutation",
            ));
        }
    };
    if receipt.request != request.key()
        || receipt.received_at_ms > request.mutation.expires_at_ms
        || receipt.retry_until_ms < request.mutation.expires_at_ms
        || receipt.retry_until_ms < receipt.received_at_ms
    {
        return Err(validation::invalid(
            "Backend receipt changed the request identity or retention guarantee",
        ));
    }
    let original = state
        .mutations
        .iter_mut()
        .find(|old| old.request == *request)
        .ok_or_else(|| validation::invalid("Receipt has no original session mutation"))?;
    if let SessionMutationState::Acknowledged(previous) = &original.state
        && previous != &ack
    {
        return Err(validation::invalid(
            "Recovered acknowledgement changed the original durable receipt",
        ));
    }
    if !matches!(original.state, SessionMutationState::Created(_)) {
        original.state = SessionMutationState::Acknowledged(ack);
    }
    Ok(())
}
