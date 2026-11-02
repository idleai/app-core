//! Validate scope, immutable logical bindings, revisions and runtime-only ordering.

use std::collections::{BTreeMap, BTreeSet};

use editchain_core::OpId;

use super::{
    SessionContext, SessionContributor, SessionCursor, SessionError, SessionErrorCode,
    SessionGrant, SessionGrantStatus, SessionInfo, SessionInputState, SessionInputUpdate,
    SessionItemBinding, SessionMutation, SessionPromptView, SessionRetryAdvice, SessionSnapshot,
    model::State,
};

/// Recovery facts were authenticated when recorded; new reports use the current binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum InputSource {
    Snapshot,
    Runtime,
}

pub(super) fn error(code: SessionErrorCode, message: &str) -> SessionError {
    SessionError {
        code,
        message: message.into(),
        retry: SessionRetryAdvice::Never,
    }
}

pub(super) fn invalid(message: &str) -> SessionError {
    error(SessionErrorCode::InvalidRequest, message)
}

pub(super) fn nonempty(value: &str) -> Result<(), SessionError> {
    if value.trim().is_empty() {
        Err(invalid("Session identities must be nonempty"))
    } else {
        Ok(())
    }
}

pub(super) fn context(value: &SessionContext) -> Result<(), SessionError> {
    for id in [
        &value.provider,
        &value.workspace_id,
        &value.contributor_id,
        &value.chain,
    ] {
        nonempty(id)?;
    }
    Ok(())
}

pub(super) fn contributor(value: &SessionContributor) -> Result<(), SessionError> {
    for id in [&value.contributor_id, &value.issuer, &value.subject] {
        nonempty(id)?;
    }
    Ok(())
}

pub(super) fn cursor(value: &SessionCursor, context: &SessionContext) -> Result<(), SessionError> {
    nonempty(&value.stream_id)?;
    if value.workspace_id != context.workspace_id || value.contributor_id != context.contributor_id
    {
        return Err(error(
            SessionErrorCode::CursorScopeMismatch,
            "Session cursor has a different workspace or audience",
        ));
    }
    Ok(())
}

pub(super) fn same_stream(first: &SessionCursor, second: &SessionCursor) -> bool {
    first.workspace_id == second.workspace_id
        && first.contributor_id == second.contributor_id
        && first.stream_id == second.stream_id
}

fn unique<'a>(ids: impl Iterator<Item = &'a str>) -> Result<(), SessionError> {
    let mut seen = BTreeSet::new();
    for id in ids {
        nonempty(id)?;
        if !seen.insert(id) {
            return Err(invalid("Duplicate identity in session snapshot"));
        }
    }
    Ok(())
}

pub(super) fn session(value: &SessionInfo, context: &SessionContext) -> Result<(), SessionError> {
    for id in [
        &value.id,
        &value.owner,
        &value.runtime.host_id,
        &value.runtime.runtime_id,
    ] {
        nonempty(id)?;
    }
    if value.revision == 0
        || value
            .history
            .as_ref()
            .is_some_and(|history| history.chain != context.chain)
    {
        return Err(invalid(
            "Session metadata has an invalid revision or logical chain binding",
        ));
    }
    if value.history.as_ref().is_some_and(|history| {
        OpId::from_display_str(&history.item)
            .is_none_or(|id| history.item.len() != 64 || id.to_string() != history.item)
    }) {
        return Err(invalid(
            "Session history requires a complete canonical logical item ID",
        ));
    }
    if let Some(parent) = &value.parent {
        nonempty(parent)?;
        if parent == &value.id {
            return Err(invalid("A session cannot be its own parent"));
        }
    }
    Ok(())
}

pub(super) fn remember(
    bindings: &mut BTreeMap<(String, String), SessionItemBinding>,
    context: &SessionContext,
    value: &SessionInfo,
) -> Result<(), SessionError> {
    session(value, context)?;
    let key = (context.workspace_id.clone(), value.id.clone());
    if bindings
        .get(&key)
        .is_some_and(|old| Some(old) != value.history.as_ref())
        || bindings
            .iter()
            .any(|(other, binding)| other != &key && Some(binding) == value.history.as_ref())
    {
        return Err(invalid(
            "A session's explicit logical item binding cannot be changed or reused",
        ));
    }
    if let Some(history) = &value.history {
        drop(bindings.insert(key, history.clone()));
    }
    Ok(())
}

pub(super) fn session_revision(old: &SessionInfo, next: &SessionInfo) -> Result<(), SessionError> {
    if next.revision < old.revision || (next.revision == old.revision && next != old) {
        return Err(invalid("Session metadata revision regressed or conflicts"));
    }
    if old.history.is_some() && old.history != next.history {
        return Err(invalid("Session logical item binding changed"));
    }
    Ok(())
}

pub(super) fn grant(value: &SessionGrant) -> Result<(), SessionError> {
    for id in [
        &value.id,
        &value.session_id,
        &value.grantee,
        &value.granted_by,
    ] {
        nonempty(id)?;
    }
    if value.revision == 0
        || value.permissions.is_empty()
        || value
            .permissions
            .iter()
            .enumerate()
            .any(|(index, permission)| {
                value
                    .permissions
                    .iter()
                    .take(index)
                    .any(|other| other == permission)
            })
    {
        return Err(invalid(
            "Session grant requires a positive revision and unique permissions",
        ));
    }
    if let SessionGrantStatus::Revoked { by, .. } = &value.status {
        nonempty(by)?;
    }
    Ok(())
}

pub(super) fn grant_revision(old: &SessionGrant, next: &SessionGrant) -> Result<(), SessionError> {
    if next.revision < old.revision
        || (next.revision == old.revision && next != old)
        || old.session_id != next.session_id
        || old.grantee != next.grantee
        || old.granted_by != next.granted_by
        || old.permissions != next.permissions
        || old.expires_at_ms != next.expires_at_ms
        || (matches!(old.status, SessionGrantStatus::Revoked { .. }) && old.status != next.status)
    {
        return Err(invalid(
            "Participation grant regressed, changed identity or revived a revocation",
        ));
    }
    Ok(())
}

pub(super) fn snapshot(value: &SessionSnapshot, state: &State) -> Result<(), SessionError> {
    if state.context.as_ref() != Some(&value.context)
        || value.contributor.contributor_id != value.context.contributor_id
    {
        return Err(invalid(
            "Session snapshot does not match the pending authenticated context",
        ));
    }
    context(&value.context)?;
    contributor(&value.contributor)?;
    cursor(&value.cursor, &value.context)?;
    unique(value.sessions.iter().map(|session| session.id.as_str()))?;
    unique(value.grants.iter().map(|grant| grant.id.as_str()))?;
    unique(
        value
            .members
            .iter()
            .map(|member| member.contributor_id.as_str()),
    )?;
    for session in &value.sessions {
        self::session(session, &value.context)?;
    }
    for grant in &value.grants {
        self::grant(grant)?;
    }
    if value.members.iter().any(|member| member.revision == 0) {
        return Err(invalid("Membership revisions must be positive"));
    }
    transcripts(value)?;
    if let Some(old) = &state.snapshot {
        if value.contributor != old.contributor {
            return Err(invalid(
                "Authenticated identity changed without reconnecting",
            ));
        }
        if same_stream(&old.cursor, &value.cursor) && value.cursor.position < old.cursor.position {
            return Err(invalid(
                "Session snapshot recovery boundary moved backwards",
            ));
        }
    }
    for session in &value.sessions {
        if let Some(previous) = state.known_sessions.get(&session.id) {
            session_revision(previous, session)?;
        }
    }
    for grant in &value.grants {
        if let Some(previous) = state.known_grants.get(&grant.id) {
            grant_revision(previous, grant)?;
        }
    }
    for member in &value.members {
        member_revision(state.known_members.get(&member.contributor_id), member)?;
    }
    Ok(())
}

fn transcripts(snapshot: &SessionSnapshot) -> Result<(), SessionError> {
    let mut seen = BTreeSet::new();
    for output in &snapshot.transcripts {
        nonempty(&output.turn_id)?;
        if !seen.insert((&output.session_id, &output.turn_id))
            || !snapshot
                .sessions
                .iter()
                .any(|session| session.id == output.session_id)
            || output.input.as_ref().is_some_and(|input| {
                input.session_id != output.session_id
                    || !snapshot.inputs.iter().any(|update| &update.input == input)
            })
        {
            return Err(invalid(
                "Runtime output has an invalid session or prompt mapping",
            ));
        }
    }
    Ok(())
}

fn rank(state: &SessionInputState) -> u8 {
    match state {
        SessionInputState::Rejected { .. } => 0,
        SessionInputState::Accepted { .. } => 1,
        SessionInputState::Ordered(_) => 2,
        SessionInputState::Running { .. } => 3,
        SessionInputState::Completed { .. } => 4,
    }
}

fn input_transition(
    old: &SessionInputUpdate,
    next: &SessionInputUpdate,
) -> Result<bool, SessionError> {
    if old.contributor != next.contributor {
        return Err(invalid("Runtime input contributor changed"));
    }
    if next.revision < old.revision {
        return Ok(false);
    }
    if next.revision == old.revision {
        let mut next = next.clone();
        if next.text.is_none() {
            next.text.clone_from(&old.text);
        }
        let mut enriched = old.clone();
        if enriched.text.is_none() {
            enriched.text.clone_from(&next.text);
        }
        return if enriched == next {
            Ok(old.text != next.text)
        } else {
            Err(invalid("Conflicting runtime facts at one input revision"))
        };
    }
    if matches!(
        old.state,
        SessionInputState::Rejected { .. } | SessionInputState::Completed { .. }
    ) && next.state != old.state
    {
        return Err(invalid("Terminal runtime input state cannot change"));
    }
    if rank(&next.state) < rank(&old.state)
        || old
            .state
            .delivery()
            .is_some_and(|delivery| next.state.delivery() != Some(delivery))
    {
        return Err(invalid(
            "Runtime input state or assigned delivery order regressed",
        ));
    }
    if let SessionInputState::Accepted { at_ms } = old.state {
        let accepted = match &next.state {
            SessionInputState::Accepted { at_ms } => Some(*at_ms),
            SessionInputState::Rejected { .. } => None,
            SessionInputState::Ordered(delivery)
            | SessionInputState::Running { delivery, .. }
            | SessionInputState::Completed { delivery, .. } => Some(delivery.accepted_at_ms),
        };
        if accepted != Some(at_ms) {
            return Err(invalid("Runtime acceptance time changed"));
        }
    }
    if let (
        SessionInputState::Running {
            started_at_ms: old, ..
        },
        SessionInputState::Running {
            started_at_ms: next,
            ..
        },
    ) = (&old.state, &next.state)
        && old != next
    {
        return Err(invalid("Runtime start time changed"));
    }
    Ok(true)
}

pub(super) fn input(
    state: &mut State,
    update: SessionInputUpdate,
    source: InputSource,
) -> Result<(), SessionError> {
    let context = state
        .context
        .as_ref()
        .ok_or_else(|| invalid("Runtime input requires a session context"))?;
    let session = state
        .session(&update.input.session_id)
        .ok_or_else(|| invalid("Runtime input references an unknown session"))?;
    contributor(&update.contributor)?;
    nonempty(&update.input.request.request_id)?;
    nonempty(&update.runtime_id)?;
    if update.input.request.workspace_id != context.workspace_id
        || update.contributor.contributor_id != update.input.request.contributor_id
        || update.revision == 0
    {
        return Err(invalid(
            "Runtime input scope, contributor or revision is invalid",
        ));
    }
    if let Some(mutation) = state
        .mutations
        .iter()
        .find(|mutation| mutation.request.key() == update.input.request)
        && (mutation.request.contributor != update.contributor
            || !matches!(&mutation.mutation, SessionMutation::Submit { session_id, .. } if *session_id == update.input.session_id))
    {
        return Err(invalid(
            "Runtime update does not match the original attributed submission",
        ));
    }
    let previous = state
        .prompts
        .iter()
        .find(|prompt| prompt.input.request == update.input.request);
    if let Some(previous) = previous {
        if update.text.as_ref().is_some_and(|text| {
            text.len() > 32 * 1024 || previous.text.as_ref().is_some_and(|old| old != text)
        }) {
            return Err(invalid(
                "Runtime prompt text conflicts with the original submission",
            ));
        }
        if previous.input != update.input || previous.contributor != update.contributor {
            return Err(invalid(
                "Runtime input reused a request key for different input",
            ));
        }
        if let Some(old) = &previous.runtime
            && !input_transition(old, &update)?
        {
            return Ok(());
        }
    }
    if source == InputSource::Runtime && update.runtime_id != session.runtime.runtime_id {
        return Err(invalid(
            "Input update is not from the session's bound runtime",
        ));
    }
    if let Some(delivery) = update.state.delivery() {
        if delivery.order == 0 || delivery.ordered_at_ms < delivery.accepted_at_ms {
            return Err(invalid(
                "Runtime delivery order or acceptance interval is invalid",
            ));
        }
        if state.prompts.iter().any(|prompt| {
            prompt.input.session_id == update.input.session_id
                && prompt.input != update.input
                && prompt
                    .runtime
                    .as_ref()
                    .and_then(|runtime| runtime.state.delivery())
                    .is_some_and(|old| old.order == delivery.order)
        }) {
            return Err(invalid(
                "Two runtime inputs claim the same session delivery order",
            ));
        }
    }
    if !state.can_observe(&update.input.session_id)
        && previous.is_none_or(|prompt| prompt.text.is_none())
    {
        return Ok(());
    }
    if let Some(prompt) = state
        .prompts
        .iter_mut()
        .find(|prompt| prompt.input == update.input)
    {
        if prompt.text.is_none() {
            prompt.text.clone_from(&update.text);
        }
        prompt.runtime = Some(update);
    } else {
        state.prompts.push(SessionPromptView {
            input: update.input.clone(),
            contributor: update.contributor.clone(),
            text: update.text.clone(),
            submission: None,
            runtime: Some(update),
        });
    }
    Ok(())
}

pub(super) fn member_revision(
    previous: Option<&crate::workspace::MemberInfo>,
    next: &crate::workspace::MemberInfo,
) -> Result<(), SessionError> {
    nonempty(&next.contributor_id)?;
    if next.revision == 0
        || previous.is_some_and(|old| {
            next.revision < old.revision
                || (next.revision == old.revision
                    && (next.role != old.role || next.status != old.status))
        })
    {
        return Err(invalid("Membership revision regressed or conflicts"));
    }
    Ok(())
}
