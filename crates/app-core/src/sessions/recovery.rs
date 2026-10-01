//! Atomic session snapshots and retained coordination/runtime changes.

use crux_core::render;

use super::{
    Model, SessionAction, SessionChange, SessionChanges, SessionError, SessionErrorCode,
    SessionLoadState, SessionOperation, SessionResult, SessionSnapshot,
    model::State,
    reducer::{SessionCommand, refresh, retire_inaccessible, watch},
    validation,
};

pub(super) fn accept(
    model: &mut Model,
    operation: &SessionOperation,
    result: SessionResult,
) -> Result<SessionCommand, SessionError> {
    match (&operation.action, result) {
        (SessionAction::Snapshot, SessionResult::Snapshot(snapshot)) => {
            accept_snapshot(model, *snapshot)
        }
        (SessionAction::Watch { after }, SessionResult::Changes(changes))
            if *after == changes.after =>
        {
            accept_changes(model, changes)
        }
        (SessionAction::Watch { .. }, SessionResult::SnapshotRequired) => Ok(refresh(model)),
        _ => Err(validation::invalid(
            "Session response does not match the pending read",
        )),
    }
}

fn active_audience(state: &State) -> Result<(), SessionError> {
    if state
        .context
        .as_ref()
        .is_none_or(|context| !state.member_active(&context.contributor_id))
    {
        Err(validation::error(
            SessionErrorCode::Forbidden,
            "Current contributor has no active workspace membership",
        ))
    } else {
        Ok(())
    }
}

fn accept_snapshot(
    model: &mut Model,
    snapshot: SessionSnapshot,
) -> Result<SessionCommand, SessionError> {
    validation::snapshot(&snapshot, &model.state)?;
    let mut bindings = model.bindings.clone();
    for session in &snapshot.sessions {
        validation::remember(&mut bindings, &snapshot.context, session)?;
    }
    let mut state = model.state.clone();
    let inputs = snapshot.inputs.clone();
    for (index, update) in inputs.iter().enumerate() {
        if inputs
            .iter()
            .take(index)
            .any(|previous| previous.input.request == update.input.request)
        {
            return Err(validation::invalid(
                "Duplicate input key in session snapshot",
            ));
        }
    }
    state.now_ms = state.now_ms.max(snapshot.now_ms);
    state.known_sessions.extend(
        snapshot
            .sessions
            .iter()
            .map(|session| (session.id.clone(), session.clone())),
    );
    state.known_grants.extend(
        snapshot
            .grants
            .iter()
            .map(|grant| (grant.id.clone(), grant.clone())),
    );
    state.known_members.extend(
        snapshot
            .members
            .iter()
            .map(|member| (member.contributor_id.clone(), member.clone())),
    );
    state.snapshot = Some(snapshot);
    if let Err(error) = active_audience(&state) {
        model.state = state;
        return Err(error);
    }
    state.prompts.retain(|prompt| {
        prompt.text.is_some() || inputs.iter().any(|update| update.input == prompt.input)
    });
    for input in inputs {
        validation::input(&mut state, input)?;
    }
    state.load = SessionLoadState::Ready;
    state.updates = SessionLoadState::Ready;
    state.action_error = None;
    state.prune();
    model.state = state;
    model.bindings = bindings;
    retire_inaccessible(model);
    Ok(watch(model))
}

fn accept_changes(
    model: &mut Model,
    changes: SessionChanges,
) -> Result<SessionCommand, SessionError> {
    let snapshot = model
        .state
        .snapshot
        .as_ref()
        .ok_or_else(|| validation::invalid("Session changes require a snapshot"))?;
    if snapshot.cursor != changes.after
        || !validation::same_stream(&changes.after, &changes.through)
        || changes.through.position < changes.after.position
    {
        return Err(validation::invalid(
            "Session change page has an invalid recovery boundary",
        ));
    }
    let mut previous = changes.after.position;
    let mut state = model.state.clone();
    let mut bindings = model.bindings.clone();
    for event in changes.events {
        if event.position <= previous || event.position > changes.through.position {
            return Err(validation::invalid(
                "Session events must increase within their scanned boundary",
            ));
        }
        previous = event.position;
        if let SessionChange::Session(session) = &event.change {
            validation::remember(&mut bindings, &snapshot.context, session)?;
        }
        apply(&mut state, event.change)?;
    }
    state.now_ms = state.now_ms.max(changes.now_ms);
    if let Err(error) = active_audience(&state) {
        model.state = state;
        return Err(error);
    }
    let current = state
        .snapshot
        .as_mut()
        .ok_or_else(|| validation::invalid("Session snapshot disappeared"))?;
    current.cursor = changes.through;
    state.updates = SessionLoadState::Ready;
    state.prune();
    model.state = state;
    model.bindings = bindings;
    retire_inaccessible(model);
    Ok(watch(model).and(render::render()))
}

fn apply(state: &mut State, change: SessionChange) -> Result<(), SessionError> {
    if let SessionChange::Input(update) = change {
        return validation::input(state, update);
    }
    let snapshot = state
        .snapshot
        .as_mut()
        .ok_or_else(|| validation::invalid("Session changes require a snapshot"))?;
    match change {
        SessionChange::Session(session) => {
            validation::session(&session, &snapshot.context)?;
            if let Some(old) = state.known_sessions.get(&session.id) {
                validation::session_revision(old, &session)?;
            }
            drop(
                state
                    .known_sessions
                    .insert(session.id.clone(), session.clone()),
            );
            if let Some(old) = snapshot
                .sessions
                .iter_mut()
                .find(|old| old.id == session.id)
            {
                validation::session_revision(old, &session)?;
                *old = session;
            } else {
                snapshot.sessions.push(session);
            }
        }
        SessionChange::Grant(grant) => {
            validation::grant(&grant)?;
            if let Some(old) = state.known_grants.get(&grant.id) {
                validation::grant_revision(old, &grant)?;
            }
            drop(state.known_grants.insert(grant.id.clone(), grant.clone()));
            if let Some(old) = snapshot.grants.iter_mut().find(|old| old.id == grant.id) {
                validation::grant_revision(old, &grant)?;
                *old = grant;
            } else {
                snapshot.grants.push(grant);
            }
        }
        SessionChange::Member(member) => {
            validation::member_revision(state.known_members.get(&member.contributor_id), &member)?;
            drop(
                state
                    .known_members
                    .insert(member.contributor_id.clone(), member.clone()),
            );
            if let Some(old) = snapshot
                .members
                .iter_mut()
                .find(|old| old.contributor_id == member.contributor_id)
            {
                *old = member;
            } else {
                snapshot.members.push(member);
            }
        }
        SessionChange::Input(_) => {}
    }
    Ok(())
}
