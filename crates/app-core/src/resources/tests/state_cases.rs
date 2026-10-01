use crate::{Core, Effect, workspace::WorkspaceMode};

use super::{ready, ready_with, request, send, snapshot};
use crate::resources::{
    ControllerAssignment, ControllerPhase, Event, ModelKey, ModelProviderKind,
    ResourceAvailability, ResourceCapabilities, ResourceCapability, ResourceErrorCode,
    ResourceLoadState, ResourcePermission, ResourceResult, ResourceRuntimeInfo, ResourceScope,
    scripted,
};

#[test]
fn discovery_is_shared_in_both_modes_and_runtime_support_defaults_unavailable() {
    assert_eq!(
        Core::new().view().resources.capabilities,
        ResourceCapabilities::default(),
        "no invented runtime capability"
    );
    for mode in [WorkspaceMode::Standalone, WorkspaceMode::Managed] {
        let source = scripted::demo_snapshot(mode).expect("mode fixture");
        let core = ready_with(source.clone());
        let view = core.view().resources;
        assert_eq!(view.load, ResourceLoadState::Ready, "authorized snapshot");
        assert_eq!(
            view.context,
            Some(source.context),
            "workspace/audience retained"
        );
        assert_eq!(
            view.hosts.first().expect("host").host.owner,
            "contributor-alice",
            "resource ownership differs from caller"
        );
        assert_eq!(
            view.hosts.first().expect("host").actions,
            [
                ResourcePermission::ConnectHost,
                ResourcePermission::InstallModel
            ],
            "independent compute grants"
        );
        assert_eq!(
            view.models.first().expect("model").selectable_for.len(),
            2,
            "runner and Control are explicit targets"
        );
        assert_eq!(
            view.controller.assignment,
            ControllerAssignment::Assigned,
            "current lease"
        );
        assert_eq!(
            view.controller.phase,
            ControllerPhase::Running,
            "authenticated runtime report"
        );
    }
    let mut source = snapshot();
    source.runtime = ResourceRuntimeInfo::default();
    let view = ready_with(source).view().resources;
    assert_eq!(
        view.hosts.first().expect("host").availability,
        ResourceAvailability::Available,
        "discovery health remains separate"
    );
    assert!(
        view.hosts.iter().all(|host| host.actions.is_empty()),
        "publication cannot create runtime support"
    );
    assert_eq!(
        view.controller.assignment,
        ControllerAssignment::Assigned,
        "lease remains known"
    );
    assert_eq!(
        view.controller.phase,
        ControllerPhase::Unknown,
        "lease is not a running observation"
    );
}

#[test]
fn permissions_never_follow_ownership_or_session_access_and_membership_overrides_grants() {
    let mut source = snapshot();
    source
        .hosts
        .first_mut()
        .expect("host")
        .owner
        .clone_from(&source.context.contributor_id);
    source
        .providers
        .first_mut()
        .expect("provider")
        .owner
        .clone_from(&source.context.contributor_id);
    source.grants.clear();
    let view = ready_with(source).view().resources;
    assert!(
        view.hosts.iter().all(|host| host.actions.is_empty()),
        "ownership does not imply compute grants"
    );
    assert!(
        view.models
            .iter()
            .all(|model| model.selectable_for.is_empty()),
        "ownership does not imply provider access"
    );

    let mut source = snapshot();
    source
        .grants
        .retain(|grant| matches!(grant.scope, ResourceScope::Provider(_)));
    let view = ready_with(source).view().resources;
    assert!(
        view.hosts.first().expect("host").actions.is_empty(),
        "provider access never grants compute access"
    );
    assert!(
        !view
            .models
            .first()
            .expect("model")
            .selectable_for
            .is_empty(),
        "a peer can use a local model with a provider grant without administering its host"
    );
    assert!(
        !view.packages.first().expect("package").can_install,
        "install needs its own compute grant"
    );

    let mut source = snapshot();
    source.member_active = false;
    let view = ready_with(source).view().resources;
    assert!(
        view.hosts.iter().all(|host| host.actions.is_empty()),
        "membership revocation overrides grants"
    );
    assert!(
        view.models
            .iter()
            .all(|model| model.selectable_for.is_empty()),
        "revoked members cannot select models"
    );
}

#[test]
fn provider_qualified_models_and_local_host_health_do_not_collapse() {
    let mut source = snapshot();
    let mut external = source.providers.first().expect("provider").clone();
    external.id = "external".into();
    external.kind = ModelProviderKind::External;
    source.providers.push(external);
    let mut model = source.models.first().expect("model").clone();
    model.key.provider_id = "external".into();
    source.models.push(model);
    source.hosts.first_mut().expect("host").health.availability = ResourceAvailability::Unavailable;
    let view = ready_with(source).view().resources;
    assert_eq!(
        view.models.len(),
        2,
        "same model ID has independent provider bindings"
    );
    assert_eq!(
        view.models.first().expect("local").availability,
        ResourceAvailability::Unavailable,
        "local model requires its serving host"
    );
    assert_eq!(
        view.models.last().expect("external").availability,
        ResourceAvailability::Available,
        "external model health is independent of this compute host"
    );
    assert_eq!(view.hosts.len(), 1, "offline hosts retain identity");
}

#[test]
fn deadlines_and_suspension_disable_actions_without_inventing_controller_failover() {
    let mut source = snapshot();
    for grant in &mut source.grants {
        grant.expires_at_ms = Some(1200);
    }
    let core = ready_with(source);
    let _effects = send(&core, Event::AdvanceClock(1200));
    let view = core.view().resources;
    assert_eq!(
        view.hosts.first().expect("host").availability,
        ResourceAvailability::Available,
        "grant expiry is not health failure"
    );
    assert!(
        view.hosts.first().expect("host").actions.is_empty(),
        "grant expires at its exclusive boundary"
    );
    assert!(
        view.models
            .first()
            .expect("model")
            .selectable_for
            .is_empty(),
        "provider grant also expires"
    );
    let _effects = send(&core, Event::AdvanceClock(900));
    assert!(
        core.view()
            .resources
            .hosts
            .first()
            .expect("host")
            .actions
            .is_empty(),
        "backward ticks cannot revive grants"
    );
    let _effects = send(&core, Event::AdvanceClock(2500));
    assert_eq!(
        core.view().resources.controller.phase,
        ControllerPhase::Unknown,
        "expired runtime health cannot claim running"
    );
    let epoch = core.view().resources.controller.ownership.last_epoch;
    let _effects = send(&core, Event::AdvanceClock(3000));
    assert_eq!(
        core.view().resources.controller.assignment,
        ControllerAssignment::Expired,
        "lease deadline is exclusive"
    );
    assert_eq!(
        core.view().resources.controller.ownership.last_epoch,
        epoch,
        "expiry cannot elect another controller"
    );
    let _effects = send(&core, Event::Suspend);
    assert_eq!(
        core.view().resources.controller.assignment,
        ControllerAssignment::Unknown,
        "lost continuity makes assignment stale"
    );
}

#[test]
fn navigation_is_local_and_detachment_prunes_only_the_current_workspace() {
    let core = ready();
    for event in [
        Event::SelectHost(Some("host-shared".into())),
        Event::SelectProvider(Some("provider-local".into())),
    ] {
        assert!(
            send(&core, event)
                .iter()
                .all(|effect| matches!(effect, Effect::Render(_))),
            "navigation performs no runtime operation"
        );
    }
    let other = ready();
    let mut load = request(send(&core, Event::Refresh));
    let mut source = snapshot();
    source.hosts.clear();
    source.providers.clear();
    source.models.clear();
    source.runtime = ResourceRuntimeInfo::default();
    let _effects = core
        .resolve(&mut load, Ok(ResourceResult::Snapshot(Box::new(source))))
        .expect("detachment replacement");
    let view = core.view().resources;
    assert!(
        view.selected_host.is_none() && view.selected_provider.is_none(),
        "detached navigation is pruned"
    );
    assert_eq!(
        other.view().resources.hosts.len(),
        1,
        "independent clients/workspace bindings remain unchanged"
    );
    let _effects = send(&core, Event::SelectHost(Some("absent".into())));
    assert_eq!(
        core.view()
            .resources
            .action_error
            .expect("invalid selection")
            .code,
        ResourceErrorCode::InvalidSelection,
        "unknown host cannot be selected"
    );
}

#[test]
fn replacements_are_atomic_and_reject_conflicts_foreign_context_and_stale_epochs() {
    for change in 0..5 {
        let core = ready();
        let mut load = request(send(&core, Event::Refresh));
        let mut source = snapshot();
        match change {
            0 => source.context.contributor_id = "someone-else".into(),
            1 => source
                .hosts
                .push(source.hosts.first().expect("host").clone()),
            2 => {
                source.providers.first_mut().expect("provider").name =
                    "changed without revision".into();
            }
            3 => source.controller.last_epoch = 1,
            _ => {
                source
                    .runtime
                    .controller
                    .as_mut()
                    .expect("controller")
                    .target
                    .runtime_id = "wrong".into();
            }
        }
        let _effects = core
            .resolve(&mut load, Ok(ResourceResult::Snapshot(Box::new(source))))
            .expect("host response");
        let view = core.view().resources;
        assert!(
            matches!(view.load, ResourceLoadState::Failed(_)),
            "invalid replacements fail atomically"
        );
        assert_eq!(
            view.hosts.len(),
            1,
            "prior rows retained on invalid refresh"
        );
        assert!(
            view.hosts.first().expect("host").actions.is_empty(),
            "stale rows cannot authorize actions"
        );
        assert_eq!(
            view.hosts.first().expect("host").availability,
            ResourceAvailability::Unknown,
            "retained health is stale"
        );
    }
}

#[test]
fn runtime_selected_model_is_retained_even_when_the_publication_disappears() {
    let mut source = snapshot();
    let selected = ModelKey {
        provider_id: "removed-provider".into(),
        model_id: "old-model".into(),
    };
    source
        .runtime
        .selections
        .first_mut()
        .expect("selection")
        .selected = Some(selected.clone());
    let core = ready_with(source);
    assert_eq!(
        core.view()
            .resources
            .selections
            .first()
            .expect("selection")
            .selected,
        Some(selected),
        "discovery cannot invent a replacement choice"
    );
    let _effects = send(&core, Event::Suspend);
    assert_eq!(
        core.view()
            .resources
            .selections
            .first()
            .expect("selection")
            .capability,
        ResourceCapability::Unavailable,
        "stale selections expose no action capability"
    );
}

#[test]
fn a_current_controller_failure_remains_visible_while_its_runtime_is_unavailable() {
    let mut source = snapshot();
    let runtime = source.runtime.controller.as_mut().expect("controller");
    runtime.phase = ControllerPhase::Failed("Model failed to start".into());
    runtime.health.availability = ResourceAvailability::Unavailable;
    let core = ready_with(source);
    assert_eq!(
        core.view().resources.controller.phase,
        ControllerPhase::Failed("Model failed to start".into()),
        "current failure details are not erased by unhealthy status"
    );
    assert_eq!(
        core.view().resources.controller.availability,
        ResourceAvailability::Unavailable,
        "failure does not claim availability"
    );
    let _effects = send(&core, Event::AdvanceClock(2500));
    assert_eq!(
        core.view().resources.controller.phase,
        ControllerPhase::Unknown,
        "expired failure observation is no longer current"
    );
}

#[test]
fn omitting_a_revoked_grant_cannot_reactivate_it_in_a_later_replacement() {
    let mut source = snapshot();
    for grant in &mut source.grants {
        grant.active = false;
        grant.revision = 2;
    }
    let core = ready_with(source.clone());
    let mut omitted = source.clone();
    omitted.grants.clear();
    let mut load = request(send(&core, Event::Refresh));
    let _effects = core
        .resolve(&mut load, Ok(ResourceResult::Snapshot(Box::new(omitted))))
        .expect("replacement omits revoked grants");
    for grant in &mut source.grants {
        grant.active = true;
        grant.revision = 3;
    }
    let mut load = request(send(&core, Event::Refresh));
    let _effects = core
        .resolve(&mut load, Ok(ResourceResult::Snapshot(Box::new(source))))
        .expect("invalid reactivation response");
    assert!(
        matches!(core.view().resources.load, ResourceLoadState::Failed(_)),
        "grant revocation survives omission within the same context"
    );
    assert!(
        core.view()
            .resources
            .hosts
            .first()
            .expect("host")
            .actions
            .is_empty(),
        "old grant identities cannot enable actions again"
    );
}
