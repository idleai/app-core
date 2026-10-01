//! Validate complete replacements before making any of their actions visible.

use std::collections::BTreeSet;

use super::{
    ControllerOwnership, ModelTarget, ResourceContext, ResourceError, ResourceErrorCode,
    ResourceHealth, ResourcePermission, ResourceRetryAdvice, ResourceScope, ResourceSnapshot,
};

pub(super) fn invalid(message: &str) -> ResourceError {
    ResourceError {
        code: ResourceErrorCode::InvalidData,
        message: message.into(),
        retry: ResourceRetryAdvice::Never,
    }
}

pub(super) fn selection(message: &str) -> ResourceError {
    ResourceError {
        code: ResourceErrorCode::InvalidSelection,
        message: message.into(),
        retry: ResourceRetryAdvice::Never,
    }
}

pub(super) fn uncertain(message: &str) -> ResourceError {
    ResourceError {
        code: ResourceErrorCode::Unavailable,
        message: message.into(),
        retry: ResourceRetryAdvice::QueryStatus,
    }
}

pub(super) fn context(value: &ResourceContext) -> Result<(), ResourceError> {
    if [
        &value.provider,
        &value.workspace_id,
        &value.contributor_id,
        &value.chain,
    ]
    .iter()
    .any(|part| part.is_empty())
    {
        return Err(invalid(
            "Resource context requires provider, workspace, contributor and chain",
        ));
    }
    Ok(())
}

fn ids<'a>(values: impl Iterator<Item = &'a str>) -> Result<(), ResourceError> {
    let mut seen = BTreeSet::new();
    for id in values {
        if id.is_empty() || !seen.insert(id) {
            return Err(invalid("Empty or duplicate resource identity"));
        }
    }
    Ok(())
}

fn health(value: &ResourceHealth) -> Result<(), ResourceError> {
    if value.observed_at_ms >= value.valid_until_ms {
        return Err(invalid(
            "Resource health requires an exclusive freshness deadline",
        ));
    }
    Ok(())
}

fn target(value: &ModelTarget) -> Result<(), ResourceError> {
    if [&value.session_id, &value.host_id, &value.runtime_id]
        .iter()
        .any(|id| id.is_empty())
        || value.control_epoch == Some(0)
    {
        return Err(invalid(
            "Model target requires exact session, host, runtime and valid Control epoch",
        ));
    }
    Ok(())
}

pub(super) fn snapshot(
    value: &ResourceSnapshot,
    scope: &ResourceContext,
    previous: Option<&ResourceSnapshot>,
) -> Result<(), ResourceError> {
    context(&value.context)?;
    if &value.context != scope || value.stream_id.is_empty() {
        return Err(invalid(
            "Resource replacement belongs to a different connection or has no stream generation",
        ));
    }
    ids(value.hosts.iter().map(|host| host.id.as_str()))?;
    ids(value.providers.iter().map(|provider| provider.id.as_str()))?;
    ids(value.grants.iter().map(|grant| grant.id.as_str()))?;
    let mut models = BTreeSet::new();
    for model in &value.models {
        if model.key.provider_id.is_empty()
            || model.key.model_id.is_empty()
            || model.revision == 0
            || !models.insert(&model.key)
        {
            return Err(invalid(
                "Invalid provider-qualified model identity or revision",
            ));
        }
        health(&model.health)?;
    }
    for host in &value.hosts {
        if host.owner.is_empty() || host.revision == 0 {
            return Err(invalid("Host owner and revision are required"));
        }
        health(&host.health)?;
    }
    for provider in &value.providers {
        if provider.owner.is_empty() || provider.revision == 0 {
            return Err(invalid("Provider owner and revision are required"));
        }
        if let super::ModelProviderKind::Local {
            host_id,
            runtime_id,
        } = &provider.kind
            && (host_id.is_empty() || runtime_id.is_empty())
        {
            return Err(invalid("Local provider requires a host and runtime"));
        }
        health(&provider.health)?;
    }
    for grant in &value.grants {
        let (id, valid) = match &grant.scope {
            ResourceScope::Host(id) => (
                id,
                grant.permissions.iter().all(|permission| {
                    matches!(
                        permission,
                        ResourcePermission::ConnectHost | ResourcePermission::InstallModel
                    )
                }),
            ),
            ResourceScope::Provider(id) => (
                id,
                grant
                    .permissions
                    .iter()
                    .all(|permission| *permission == ResourcePermission::UseModels),
            ),
        };
        if id.is_empty() || grant.revision == 0 || !valid {
            return Err(invalid(
                "Grant permissions do not match their resource scope",
            ));
        }
    }
    ownership(&value.controller)?;
    if let Some(runtime) = &value.runtime.controller {
        health(&runtime.health)?;
        if !value
            .controller
            .lease
            .as_ref()
            .is_some_and(|lease| lease.target == runtime.target)
        {
            return Err(invalid(
                "Controller observation does not match the assigned holder and epoch",
            ));
        }
    }
    ids(value
        .runtime
        .selections
        .iter()
        .map(|selection| selection.target.session_id.as_str()))?;
    for selection in &value.runtime.selections {
        target(&selection.target)?;
        if selection.target.control_epoch.is_some()
            && !value
                .controller
                .lease
                .as_ref()
                .is_some_and(|lease| lease.target == selection.target)
        {
            return Err(invalid(
                "Control model choice does not match current ownership",
            ));
        }
        if selection
            .selected
            .as_ref()
            .is_some_and(|key| key.provider_id.is_empty() || key.model_id.is_empty())
        {
            return Err(invalid(
                "Selected models require provider-qualified identities",
            ));
        }
    }
    let mut packages = BTreeSet::new();
    for package in &value.runtime.packages {
        if [&package.host_id, &package.runtime_id, &package.package_id]
            .iter()
            .any(|id| id.is_empty())
            || !packages.insert((&package.host_id, &package.runtime_id, &package.package_id))
        {
            return Err(invalid("Invalid or duplicate runtime installation option"));
        }
    }
    if let Some(previous) = previous {
        replacement(value, previous)?;
    }
    Ok(())
}

fn ownership(value: &ControllerOwnership) -> Result<(), ResourceError> {
    if let Some(lease) = &value.lease {
        target(&lease.target)?;
        if lease.target.control_epoch != Some(value.last_epoch)
            || value.last_epoch == 0
            || lease.acquired_at_ms >= lease.expires_at_ms
        {
            return Err(invalid(
                "Controller lease has an invalid epoch or validity window",
            ));
        }
    }
    Ok(())
}

fn replacement(value: &ResourceSnapshot, previous: &ResourceSnapshot) -> Result<(), ResourceError> {
    if value.now_ms < previous.now_ms
        || (value.stream_id == previous.stream_id && value.position < previous.position)
        || value.controller.last_epoch < previous.controller.last_epoch
    {
        return Err(invalid(
            "Resource replacement moves its clock, cursor or controller epoch backwards",
        ));
    }
    if value.controller.last_epoch == previous.controller.last_epoch
        && let Some(lease) = &value.controller.lease
    {
        let Some(old) = &previous.controller.lease else {
            return Err(invalid(
                "Controller reacquisition must increase the ownership epoch",
            ));
        };
        if old.target != lease.target
            || old.acquired_at_ms != lease.acquired_at_ms
            || lease.expires_at_ms < old.expires_at_ms
        {
            return Err(invalid("Controller holder changed without a new epoch"));
        }
    }
    for host in &value.hosts {
        if let Some(old) = previous.hosts.iter().find(|old| old.id == host.id) {
            revision(host.revision, old.revision, host == old)?;
        }
    }
    for provider in &value.providers {
        if let Some(old) = previous.providers.iter().find(|old| old.id == provider.id) {
            revision(provider.revision, old.revision, provider == old)?;
        }
    }
    for model in &value.models {
        if let Some(old) = previous.models.iter().find(|old| old.key == model.key) {
            revision(model.revision, old.revision, model == old)?;
        }
    }
    for grant in &value.grants {
        if let Some(old) = previous.grants.iter().find(|old| old.id == grant.id) {
            grant_update(grant, old)?;
        }
    }
    Ok(())
}

pub(super) fn grant_update(
    value: &super::ResourceGrant,
    previous: &super::ResourceGrant,
) -> Result<(), ResourceError> {
    revision(value.revision, previous.revision, value == previous)?;
    if (!previous.active && value.active) || previous.scope != value.scope {
        return Err(invalid("Resource grant revocation and scope are immutable"));
    }
    Ok(())
}

pub(super) fn revision(current: u64, previous: u64, identical: bool) -> Result<(), ResourceError> {
    if current < previous || (current == previous && !identical) {
        return Err(invalid("Conflicting or regressing resource revision"));
    }
    Ok(())
}
