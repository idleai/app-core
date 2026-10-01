//! Reject inconsistent provider data before it changes selected context.

use std::collections::{BTreeMap, BTreeSet};

use super::{
    MemberStatus, PresenceSnapshot, WorkspaceError, WorkspaceErrorKind, WorkspaceInfo,
    WorkspaceMode, WorkspaceSnapshot,
};

pub(super) fn invalid(message: &str) -> WorkspaceError {
    WorkspaceError {
        kind: WorkspaceErrorKind::InvalidData,
        message: message.into(),
    }
}

pub(super) fn unique_ids<'a>(ids: impl Iterator<Item = &'a str>) -> Result<(), WorkspaceError> {
    let mut seen = BTreeSet::new();
    for id in ids {
        if id.trim().is_empty() || !seen.insert(id) {
            return Err(invalid("Empty or duplicate identity in workspace metadata"));
        }
    }
    Ok(())
}

fn workspace(info: &WorkspaceInfo) -> Result<(), WorkspaceError> {
    if info.id.trim().is_empty() || info.chain.trim().is_empty() {
        return Err(invalid(
            "Workspace and logical chain references must be nonempty",
        ));
    }
    if info.mode == WorkspaceMode::Standalone && info.repositories.len() != 1 {
        return Err(invalid(
            "Standalone coordination requires exactly one repository",
        ));
    }
    unique_ids(info.repositories.iter().map(|repo| repo.id.as_str()))
}

pub(super) fn remember(
    known: &mut BTreeMap<String, WorkspaceInfo>,
    info: &WorkspaceInfo,
) -> Result<(), WorkspaceError> {
    workspace(info)?;
    if known
        .values()
        .any(|entry| entry.id != info.id && entry.chain == info.chain)
    {
        return Err(invalid("A logical chain cannot belong to two workspaces"));
    }
    if let Some(previous) = known.get(&info.id) {
        if info.chain != previous.chain {
            return Err(invalid("A workspace's logical chain binding is immutable"));
        }
        if info.revision < previous.revision {
            return Err(invalid("Workspace metadata revision moved backwards"));
        }
        if info.revision == previous.revision && info != previous {
            return Err(invalid(
                "Conflicting workspace metadata at the same revision",
            ));
        }
    }
    let _previous = known.insert(info.id.clone(), info.clone());
    Ok(())
}

pub(super) fn snapshot(snapshot: &WorkspaceSnapshot) -> Result<(), WorkspaceError> {
    workspace(&snapshot.workspace)?;
    unique_ids(
        snapshot
            .members
            .iter()
            .map(|entry| entry.contributor_id.as_str()),
    )?;
    unique_ids(snapshot.host_ids.iter().map(String::as_str))?;
    unique_ids(snapshot.provider_ids.iter().map(String::as_str))
}

pub(super) fn member_revisions(
    previous: &WorkspaceSnapshot,
    next: &WorkspaceSnapshot,
) -> Result<(), WorkspaceError> {
    for member in &next.members {
        if let Some(old) = previous
            .members
            .iter()
            .find(|entry| entry.contributor_id == member.contributor_id)
            && (member.revision < old.revision
                || (member.revision == old.revision
                    && (member.role != old.role || member.status != old.status)))
        {
            return Err(invalid(
                "Membership revision regressed or contains conflicting state",
            ));
        }
    }
    Ok(())
}

pub(super) fn presence(
    presence: &PresenceSnapshot,
    snapshot: &WorkspaceSnapshot,
) -> Result<(), WorkspaceError> {
    if presence.workspace_id != snapshot.workspace.id {
        return Err(invalid("Presence belongs to a different workspace"));
    }
    unique_ids(
        presence
            .entries
            .iter()
            .map(|entry| entry.connection_id.as_str()),
    )?;
    for entry in &presence.entries {
        let member = snapshot
            .members
            .iter()
            .find(|member| member.contributor_id == entry.contributor_id)
            .ok_or_else(|| invalid("Presence references an unknown workspace member"))?;
        // Revocation wins even if the presence source has not caught up yet.
        if member.status == MemberStatus::Revoked {
            continue;
        }
        if entry.observed_at_ms > presence.as_of_ms || entry.valid_until_ms <= entry.observed_at_ms
        {
            return Err(invalid(
                "Presence observation has an invalid freshness interval",
            ));
        }
        if entry.repository_id.as_ref().is_some_and(|id| {
            !snapshot
                .workspace
                .repositories
                .iter()
                .any(|repo| &repo.id == id)
        }) || (entry.repository_id.is_none() && (entry.file.is_some() || entry.branch.is_some()))
        {
            return Err(invalid(
                "Presence location is outside this workspace's repositories",
            ));
        }
        if entry
            .host_id
            .as_ref()
            .is_some_and(|id| !snapshot.host_ids.contains(id))
        {
            return Err(invalid("Presence host is not bound to this workspace"));
        }
    }
    Ok(())
}
