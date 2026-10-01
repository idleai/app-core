use super::{ready, ready_with, request, send, snapshot};
use crate::{
    Core, Effect,
    resources::{
        Event, ResourceActionStage, ResourceError, ResourceErrorCode, ResourceMutation,
        ResourceOperationKind, ResourceProgress, ResourceRecoveryAction, ResourceRequest,
        ResourceResult, ResourceRetryAdvice, ResourceRuntimeInfo,
    },
    workspace::WorkspaceMode,
};

fn identity() -> ResourceRequest {
    ResourceRequest {
        request_id: "persisted-install".into(),
        expires_at_ms: 2000,
    }
}

fn install() -> ResourceMutation {
    ResourceMutation::InstallModel(
        snapshot()
            .runtime
            .packages
            .first()
            .expect("package")
            .clone(),
    )
}

fn restore() -> Event {
    Event::Restore {
        context: snapshot().context,
        request: identity(),
        mutation: install(),
    }
}

fn no_resource(effects: &[Effect]) -> bool {
    effects
        .iter()
        .all(|effect| !matches!(effect, Effect::Resource(_)))
}

#[test]
fn client_restart_restores_expired_requests_without_executing_them_again() {
    let original = ready();
    let _pending = request(send(
        &original,
        Event::Execute {
            request: identity(),
            mutation: install(),
        },
    ));
    drop(original);
    let mut source = snapshot();
    source.now_ms = 2100;
    let core = ready_with(source.clone());
    let mut status = request(send(&core, restore()));
    assert_eq!(
        status.operation.kind,
        ResourceOperationKind::Status(identity()),
        "past first-receipt deadlines still permit querying the persisted request"
    );
    let view = core.view().resources;
    let restored = view.mutations.first().expect("restored action");
    assert_eq!(
        restored.request,
        identity(),
        "the original deadline is unchanged"
    );
    assert_eq!(
        restored.mutation,
        install(),
        "the original intent is unchanged"
    );
    assert!(
        restored.pending && restored.progress.is_none(),
        "restoration cannot invent progress"
    );
    assert!(
        !view.packages.first().expect("package").can_install,
        "restored pending work blocks conflicts"
    );
    let mut load = request(
        core.resolve(
            &mut status,
            Ok(ResourceResult::Progress(ResourceProgress {
                context: source.context.clone(),
                request: identity(),
                revision: 7,
                stage: ResourceActionStage::Succeeded,
            })),
        )
        .expect("retained runtime completion"),
    );
    let _effects = core
        .resolve(&mut load, Ok(ResourceResult::Snapshot(Box::new(source))))
        .expect("fresh discovery");
    let completed = core.view().resources.mutations;
    assert!(
        !completed.first().expect("completed action").pending,
        "restored completion is visible"
    );
    assert!(
        send(&core, restore()).is_empty(),
        "a duplicate restore cannot undo completion"
    );
    assert_eq!(
        core.view().resources.mutations,
        completed,
        "duplicate restore retains runtime facts"
    );
}

#[test]
fn restoration_waits_for_discovery_and_unknown_outcomes_never_enable_execution() {
    let core = Core::new();
    let mut load = request(send(&core, Event::Connect(snapshot().context)));
    assert!(
        no_resource(&send(&core, restore())),
        "restoration waits for authorized discovery"
    );
    let view = core.view().resources;
    let restored = view.mutations.first().expect("uncertain action");
    assert!(
        restored.pending && !restored.in_flight,
        "restoration records pending work before querying"
    );
    assert!(
        restored.error.is_some() && restored.recovery.is_empty(),
        "uncertainty is explicit while disconnected"
    );
    assert!(
        send(&core, restore()).is_empty(),
        "duplicate restores are idempotent"
    );
    let mut status = request(
        core.resolve(
            &mut load,
            Ok(ResourceResult::Snapshot(Box::new(snapshot()))),
        )
        .expect("authorized discovery"),
    );
    assert_eq!(
        status.operation.kind,
        ResourceOperationKind::Status(identity()),
        "discovery queries the original request"
    );
    let _effects = core
        .resolve(&mut status, Ok(ResourceResult::Unknown))
        .expect("unknown outcome");
    assert_eq!(
        core.view()
            .resources
            .mutations
            .first()
            .expect("action")
            .recovery,
        [ResourceRecoveryAction::CheckStatus],
        "restored uncertainty never invents execution retry advice"
    );
    assert!(
        no_resource(&send(&core, Event::Retry(identity().request_id))),
        "retry requires fresh runtime advice"
    );
    assert!(
        send(
            &core,
            Event::Execute {
                request: identity(),
                mutation: install()
            }
        )
        .is_empty(),
        "Execute cannot resubmit a restored immutable identity"
    );
}

#[test]
fn missing_publications_and_old_deadlines_do_not_prevent_restored_status_lookup() {
    let mut source = snapshot();
    source.hosts.clear();
    source.providers.clear();
    source.models.clear();
    source.grants.clear();
    source.runtime = ResourceRuntimeInfo::default();
    source.now_ms = 2100;
    let core = ready_with(source);
    let mut status = request(send(&core, restore()));
    assert_eq!(
        status.operation.kind,
        ResourceOperationKind::Status(identity()),
        "historical targets need not still be published"
    );
    let _effects = core
        .resolve(
            &mut status,
            Err(ResourceError {
                code: ResourceErrorCode::Unavailable,
                message: "Retry the original request".into(),
                retry: ResourceRetryAdvice::SameRequest {
                    not_before_ms: None,
                },
            }),
        )
        .expect("runtime advice");
    assert_eq!(
        core.view()
            .resources
            .mutations
            .first()
            .expect("action")
            .recovery,
        [ResourceRecoveryAction::CheckStatus],
        "restoration does not relax retry deadlines, grants or capabilities"
    );
    assert!(
        core.view().resources.hosts.is_empty(),
        "restoration cannot republish historical resources"
    );
}

#[test]
fn restored_identity_cannot_be_rebound_to_another_deadline_or_intent() {
    let core = ready();
    let _status = request(send(&core, restore()));
    let before = core.view().resources.mutations;
    for changed_deadline in [false, true] {
        let mut original = identity();
        let mutation = if changed_deadline {
            original.expires_at_ms = 3000;
            install()
        } else {
            ResourceMutation::ConnectHost {
                host_id: "host-shared".into(),
            }
        };
        assert!(
            no_resource(&send(
                &core,
                Event::Restore {
                    context: snapshot().context,
                    request: original,
                    mutation,
                }
            )),
            "restoring another payload cannot dispatch anything"
        );
        assert_eq!(
            core.view().resources.mutations,
            before,
            "existing action state is immutable"
        );
        assert!(
            core.view().resources.action_error.is_some(),
            "conflicting persisted data is rejected"
        );
    }
}

#[test]
fn restoration_rejects_every_foreign_context_component() {
    let core = ready();
    for component in 0..5 {
        let mut context = snapshot().context;
        match component {
            0 => context.provider = "different-provider".into(),
            1 => context.workspace_id = "different-workspace".into(),
            2 => context.contributor_id = "different-contributor".into(),
            3 => context.chain = "different-chain".into(),
            _ => context.mode = WorkspaceMode::Managed,
        }
        assert!(
            no_resource(&send(
                &core,
                Event::Restore {
                    context,
                    request: identity(),
                    mutation: install(),
                }
            )),
            "foreign persisted actions cannot query this connection"
        );
        assert!(
            core.view().resources.mutations.is_empty(),
            "foreign pending work cannot enter the current view"
        );
    }
    assert!(
        no_resource(&send(&Core::new(), restore())),
        "a context must be bound before restoration"
    );
}

#[test]
fn malformed_persisted_requests_cannot_enter_mutation_history() {
    for invalid in 0..6 {
        let core = ready();
        let mut original = identity();
        let mut intent = install();
        match invalid {
            0 => original.request_id.clear(),
            1 => original.expires_at_ms = 0,
            2 => {
                intent = ResourceMutation::ConnectHost {
                    host_id: String::new(),
                }
            }
            3 => {
                if let ResourceMutation::InstallModel(package) = &mut intent {
                    package.package_id.clear();
                }
            }
            _ => {
                let mut target = snapshot()
                    .runtime
                    .selections
                    .first()
                    .expect("selection")
                    .target
                    .clone();
                let mut model = snapshot().models.first().expect("model").key.clone();
                if invalid == 4 {
                    target.control_epoch = Some(0);
                } else {
                    model.provider_id.clear();
                }
                intent = ResourceMutation::SelectModel { target, model };
            }
        }
        assert!(
            no_resource(&send(
                &core,
                Event::Restore {
                    context: snapshot().context,
                    request: original,
                    mutation: intent,
                }
            )),
            "invalid persisted identities cannot dispatch status checks"
        );
        assert!(
            core.view().resources.mutations.is_empty(),
            "malformed actions are rejected atomically"
        );
    }
}
