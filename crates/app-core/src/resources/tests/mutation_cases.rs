use super::{ready, ready_with, request, send, snapshot};
use crate::resources::{
    Event, ResourceActionStage, ResourceError, ResourceErrorCode, ResourceMutation,
    ResourceOperationKind, ResourceProgress, ResourceRecoveryAction, ResourceRequest,
    ResourceResult, ResourceRetryAdvice,
};
use crate::{Core, Effect};

fn identity() -> ResourceRequest {
    ResourceRequest {
        request_id: "resource-action-1".into(),
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

fn progress(revision: u64, stage: ResourceActionStage) -> ResourceResult {
    ResourceResult::Progress(ResourceProgress {
        context: snapshot().context,
        request: identity(),
        revision,
        stage,
    })
}

fn execute(
    core: &Core,
    mutation: ResourceMutation,
) -> crux_core::Request<crate::resources::ResourceOperation> {
    request(send(
        core,
        Event::Execute {
            request: identity(),
            mutation,
        },
    ))
}

#[test]
fn receipt_and_runtime_progress_cannot_optimistically_install_or_select_a_model() {
    let core = ready();
    let original = core.view().resources;
    let target = snapshot()
        .runtime
        .selections
        .first()
        .expect("target")
        .target
        .clone();
    let selected = snapshot().models.first().expect("model").key.clone();
    let intent = ResourceMutation::SelectModel {
        target: target.clone(),
        model: selected.clone(),
    };
    let mut mutation = execute(&core, intent.clone());
    assert_eq!(
        mutation.operation.kind,
        ResourceOperationKind::Mutate {
            request: identity(),
            mutation: intent.clone()
        },
        "typed immutable effect"
    );
    assert!(
        core.view()
            .resources
            .mutations
            .first()
            .expect("pending")
            .in_flight,
        "pending before host response"
    );
    assert_eq!(
        core.view().resources.selections,
        original.selections,
        "intent cannot adopt a model"
    );
    assert!(
        send(
            &core,
            Event::Execute {
                request: identity(),
                mutation: intent
            }
        )
        .is_empty(),
        "same identity cannot execute twice"
    );
    let _effects = core
        .resolve(
            &mut mutation,
            Ok(progress(1, ResourceActionStage::Received)),
        )
        .expect("durable receipt");
    let view = core.view().resources;
    assert!(
        view.mutations.first().expect("pending").pending,
        "receipt is still pending runtime execution"
    );
    assert_eq!(
        view.selections, original.selections,
        "receipt cannot adopt a model"
    );
    let mut status = request(send(&core, Event::CheckStatus(identity().request_id)));
    assert_eq!(
        status.operation.kind,
        ResourceOperationKind::Status(identity()),
        "look up without resubmitting"
    );
    let _effects = core
        .resolve(
            &mut status,
            Ok(progress(
                2,
                ResourceActionStage::Running("Changing model".into()),
            )),
        )
        .expect("runtime progress");
    let mut status = request(send(&core, Event::CheckStatus(identity().request_id)));
    let mut load = request(
        core.resolve(&mut status, Ok(progress(3, ResourceActionStage::Succeeded)))
            .expect("runtime success"),
    );
    assert_eq!(
        load.operation.kind,
        ResourceOperationKind::Snapshot,
        "success refreshes authoritative state"
    );
    assert_eq!(
        core.view()
            .resources
            .selections
            .first()
            .expect("selection")
            .selected,
        None,
        "completion alone cannot replace the choice record"
    );
    let mut source = snapshot();
    source
        .runtime
        .selections
        .first_mut()
        .expect("selection")
        .selected = Some(selected.clone());
    let _effects = core
        .resolve(&mut load, Ok(ResourceResult::Snapshot(Box::new(source))))
        .expect("runtime-selected choice");
    let view = core.view().resources;
    assert_eq!(
        view.selections.first().expect("selection").selected,
        Some(selected),
        "snapshot carries confirmed model choice"
    );
    assert!(
        !view.mutations.first().expect("completed").pending,
        "runtime success is terminal"
    );
    assert!(
        !view
            .models
            .first()
            .expect("model")
            .selectable_for
            .contains(&target),
        "already-selected model is not offered again"
    );
}

#[test]
fn installation_and_host_connection_use_distinct_compute_actions() {
    for intent in [
        install(),
        ResourceMutation::ConnectHost {
            host_id: "host-shared".into(),
        },
    ] {
        let core = ready();
        let mut pending = execute(&core, intent.clone());
        assert_eq!(
            pending.operation.kind,
            ResourceOperationKind::Mutate {
                request: identity(),
                mutation: intent
            },
            "runtime performs the selected operation"
        );
        let _effects = core
            .resolve(
                &mut pending,
                Ok(progress(
                    1,
                    ResourceActionStage::Running("Downloading 25%".into()),
                )),
            )
            .expect("installation progress");
        let view = core.view().resources;
        assert!(
            view.mutations.first().expect("running").pending,
            "runtime progress stays pending"
        );
        assert_eq!(
            view.models.len(),
            1,
            "progress cannot fabricate a served publication"
        );
    }
    let core = ready();
    let _pending = execute(&core, install());
    assert!(
        !core
            .view()
            .resources
            .packages
            .first()
            .expect("package")
            .can_install,
        "conflicting host installation is disabled"
    );
    let mut changed = identity();
    changed.request_id = "second-install".into();
    assert!(
        send(
            &core,
            Event::Execute {
                request: changed,
                mutation: install()
            }
        )
        .iter()
        .all(|effect| matches!(effect, Effect::Render(_))),
        "a second key cannot duplicate in-flight work"
    );
    assert!(
        send(
            &core,
            Event::Execute {
                request: identity(),
                mutation: ResourceMutation::ConnectHost {
                    host_id: "host-shared".into()
                }
            }
        )
        .iter()
        .all(|effect| matches!(effect, Effect::Render(_))),
        "key cannot be reused for another mutation"
    );
}

#[test]
fn uncertain_outcomes_require_status_lookup_and_unchanged_retries_require_explicit_advice() {
    let core = ready();
    let mut pending = execute(&core, install());
    let operation = pending.operation.clone();
    let _effects = core
        .resolve(
            &mut pending,
            Err(ResourceError {
                code: ResourceErrorCode::Unavailable,
                message: "Connection lost after dispatch".into(),
                retry: ResourceRetryAdvice::QueryStatus,
            }),
        )
        .expect("uncertain failure");
    assert_eq!(
        core.view()
            .resources
            .mutations
            .first()
            .expect("pending")
            .recovery,
        [ResourceRecoveryAction::CheckStatus],
        "uncertain execution cannot be retried"
    );
    let _effects = send(&core, Event::Retry(identity().request_id));
    assert!(
        core.view().resources.action_error.is_some(),
        "unsafe retry rejected"
    );
    let mut status = request(send(&core, Event::CheckStatus(identity().request_id)));
    let _effects = core
        .resolve(&mut status, Ok(ResourceResult::Unknown))
        .expect("unknown status");
    assert_eq!(
        core.view()
            .resources
            .mutations
            .first()
            .expect("pending")
            .recovery,
        [ResourceRecoveryAction::CheckStatus],
        "missing retention cannot prove non-execution"
    );
    let mut status = request(send(&core, Event::CheckStatus(identity().request_id)));
    let _effects = core
        .resolve(
            &mut status,
            Err(ResourceError {
                code: ResourceErrorCode::Unavailable,
                message: "Safe unchanged retry".into(),
                retry: ResourceRetryAdvice::SameRequest {
                    not_before_ms: Some(1100),
                },
            }),
        )
        .expect("runtime retry advice");
    assert_eq!(
        core.view()
            .resources
            .mutations
            .first()
            .expect("pending")
            .recovery,
        [ResourceRecoveryAction::CheckStatus],
        "retry waits for the supplied deadline"
    );
    let _effects = send(&core, Event::AdvanceClock(1100));
    let retry = request(send(&core, Event::Retry(identity().request_id)));
    assert_eq!(
        retry.operation, operation,
        "request identity, deadline, scope and payload are unchanged"
    );
}

#[test]
fn expiry_revocation_and_missing_capabilities_cannot_be_bypassed_by_execute_events() {
    let mut source = snapshot();
    source.grants.clear();
    let core = ready_with(source);
    assert!(
        send(
            &core,
            Event::Execute {
                request: identity(),
                mutation: install()
            }
        )
        .iter()
        .all(|effect| matches!(effect, Effect::Render(_))),
        "client events cannot bypass supplied actions"
    );
    let core = ready();
    let _effects = send(&core, Event::AdvanceClock(2000));
    assert!(
        send(
            &core,
            Event::Execute {
                request: identity(),
                mutation: install()
            }
        )
        .iter()
        .all(|effect| matches!(effect, Effect::Render(_))),
        "expired first-receipt deadline is rejected"
    );
    let core = ready();
    let mut package = snapshot()
        .runtime
        .packages
        .first()
        .expect("package")
        .clone();
    package.package_id = "arbitrary-package".into();
    assert!(
        send(
            &core,
            Event::Execute {
                request: identity(),
                mutation: ResourceMutation::InstallModel(package)
            }
        )
        .iter()
        .all(|effect| matches!(effect, Effect::Render(_))),
        "only supported catalog packages can be installed"
    );
}

#[test]
fn late_scope_results_and_conflicting_progress_cannot_finish_pending_work() {
    let core = ready();
    let mut pending = execute(&core, install());
    let _effects = core
        .resolve(
            &mut pending,
            Ok(progress(
                2,
                ResourceActionStage::Running("Installing".into()),
            )),
        )
        .expect("runtime progress");
    let mut status = request(send(&core, Event::CheckStatus(identity().request_id)));
    let _effects = core
        .resolve(&mut status, Ok(progress(1, ResourceActionStage::Succeeded)))
        .expect("stale status response");
    let view = core.view().resources;
    let mutation = view.mutations.first().expect("mutation");
    assert!(
        mutation.pending && mutation.error.is_some(),
        "regressing progress cannot claim success"
    );
    assert_eq!(
        mutation
            .progress
            .as_ref()
            .expect("last runtime fact")
            .revision,
        2,
        "prior progress retained"
    );
    let mut status = request(send(&core, Event::CheckStatus(identity().request_id)));
    let _effects = send(&core, Event::Disconnect);
    let mut load = request(send(&core, Event::Connect(snapshot().context)));
    let _effects = core
        .resolve(&mut status, Ok(progress(3, ResourceActionStage::Succeeded)))
        .expect("retired continuation");
    assert!(
        core.view().resources.mutations.is_empty(),
        "retired action cannot affect the reconnected scope"
    );
    let _effects = core
        .resolve(
            &mut load,
            Ok(ResourceResult::Snapshot(Box::new(snapshot()))),
        )
        .expect("new resource context");
}

#[test]
fn suspend_preserves_unknown_actions_and_reconnect_queries_original_status() {
    let core = ready();
    let mut pending = execute(&core, install());
    let _effects = send(&core, Event::Suspend);
    let view = core.view().resources;
    assert!(
        view.mutations.first().expect("pending").pending,
        "disconnect is not cancellation"
    );
    assert!(
        view.mutations.first().expect("pending").recovery.is_empty(),
        "no status queries on a disconnected transport"
    );
    let _effects = core
        .resolve(
            &mut pending,
            Ok(progress(1, ResourceActionStage::Succeeded)),
        )
        .expect("late old response");
    assert!(
        core.view()
            .resources
            .mutations
            .first()
            .expect("pending")
            .progress
            .is_none(),
        "pre-reconnect responses are retired"
    );
    let mut load = request(send(&core, Event::Reconnect));
    let status = request(
        core.resolve(
            &mut load,
            Ok(ResourceResult::Snapshot(Box::new(snapshot()))),
        )
        .expect("fresh authorized state"),
    );
    assert_eq!(
        status.operation.kind,
        ResourceOperationKind::Status(identity()),
        "reconnection recovers rather than resubmits"
    );
}

#[test]
fn a_transport_error_without_retry_permission_is_not_a_terminal_runtime_fact() {
    let core = ready();
    let mut pending = execute(&core, install());
    let _effects = core
        .resolve(
            &mut pending,
            Err(ResourceError {
                code: ResourceErrorCode::Unavailable,
                message: "Runtime transport is gone".into(),
                retry: ResourceRetryAdvice::Never,
            }),
        )
        .expect("transport failure");
    let view = core.view().resources;
    let mutation = view.mutations.first().expect("uncertain action");
    assert!(
        mutation.pending,
        "transport failure cannot establish that work stopped"
    );
    assert_eq!(
        mutation.recovery,
        [ResourceRecoveryAction::CheckStatus],
        "no automatic or manual execution retry"
    );
    assert!(
        !view.packages.first().expect("package").can_install,
        "a new identity cannot duplicate uncertain work"
    );
}

#[test]
fn continuous_discovery_changes_do_not_starve_runtime_progress_recovery() {
    let core = ready();
    let mut pending = execute(&core, install());
    let _effects = core
        .resolve(&mut pending, Ok(progress(1, ResourceActionStage::Received)))
        .expect("receipt");
    let mut load = request(send(&core, Event::Refresh));
    let _effects = send(&core, Event::Refresh);
    let effects = core
        .resolve(
            &mut load,
            Ok(ResourceResult::Snapshot(Box::new(snapshot()))),
        )
        .expect("replacement during more changes");
    let mut kinds = effects.into_iter().filter_map(|effect| {
        if let Effect::Resource(request) = effect {
            Some(request.operation.kind)
        } else {
            None
        }
    });
    let first = kinds.next().expect("first resource operation");
    let second = kinds.next().expect("second resource operation");
    assert!(
        [&first, &second].contains(&&ResourceOperationKind::Status(identity())),
        "pending status advances even while discovery stays dirty"
    );
    assert!(
        [&first, &second].contains(&&ResourceOperationKind::Snapshot),
        "queued discovery still refreshes"
    );
    assert!(
        kinds.next().is_none(),
        "no duplicate reads or status requests"
    );
}
