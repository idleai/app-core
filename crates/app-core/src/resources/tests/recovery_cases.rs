use crux_core::Request;

use super::{ready, request, send, snapshot};
use crate::{
    Core, Effect,
    resources::{
        Event, ResourceActionStage, ResourceError, ResourceErrorCode, ResourceMutation,
        ResourceOperation, ResourceOperationKind, ResourceProgress, ResourceRequest,
        ResourceResult, ResourceRetryAdvice,
    },
};

fn identity() -> ResourceRequest {
    ResourceRequest {
        request_id: "notification-race".into(),
        expires_at_ms: 2000,
    }
}

fn start(core: &Core) -> Request<ResourceOperation> {
    request(send(
        core,
        Event::Execute {
            request: identity(),
            mutation: ResourceMutation::InstallModel(
                snapshot()
                    .runtime
                    .packages
                    .first()
                    .expect("package")
                    .clone(),
            ),
        },
    ))
}

fn progress(revision: u64, stage: ResourceActionStage) -> ResourceResult {
    ResourceResult::Progress(ResourceProgress {
        context: snapshot().context,
        request: identity(),
        revision,
        stage,
    })
}

fn replace(core: &Core, load: &mut Request<ResourceOperation>) -> Vec<Effect> {
    core.resolve(load, Ok(ResourceResult::Snapshot(Box::new(snapshot()))))
        .expect("discovery response")
}

fn resource_requests(effects: Vec<Effect>) -> Vec<Request<ResourceOperation>> {
    effects
        .into_iter()
        .filter_map(|effect| {
            if let Effect::Resource(request) = effect {
                Some(*request)
            } else {
                None
            }
        })
        .collect()
}

fn one_status(effects: Vec<Effect>) -> Request<ResourceOperation> {
    let mut requests = resource_requests(effects);
    assert_eq!(requests.len(), 1, "one coalesced status check");
    let request = requests.pop().expect("status request");
    assert_eq!(
        request.operation.kind,
        ResourceOperationKind::Status(identity()),
        "query the original identity without resubmitting execution"
    );
    request
}

#[test]
fn notifications_during_mutations_and_status_queries_survive_either_response_order() {
    for query_in_flight in [false, true] {
        for action_response_first in [false, true] {
            let core = ready();
            let mut pending = start(&core);
            if query_in_flight {
                let _effects = core
                    .resolve(&mut pending, Ok(progress(1, ResourceActionStage::Received)))
                    .expect("routing receipt");
                pending = request(send(&core, Event::CheckStatus(identity().request_id)));
            }
            let stage = if query_in_flight {
                ResourceActionStage::Running("Installing".into())
            } else {
                ResourceActionStage::Received
            };
            let mut load = request(send(&core, Event::Refresh));
            let effects = if action_response_first {
                let effects = core
                    .resolve(&mut pending, Ok(progress(2, stage)))
                    .expect("older action result");
                assert!(
                    resource_requests(effects).is_empty(),
                    "status recovery waits for current discovery"
                );
                replace(&core, &mut load)
            } else {
                assert!(
                    resource_requests(replace(&core, &mut load)).is_empty(),
                    "an outstanding request cannot be duplicated"
                );
                core.resolve(&mut pending, Ok(progress(2, stage)))
                    .expect("older action result")
            };
            let mut status = one_status(effects);
            let mut load = request(
                core.resolve(&mut status, Ok(progress(3, ResourceActionStage::Succeeded)))
                    .expect("latest runtime completion"),
            );
            let _effects = replace(&core, &mut load);
            let view = core.view().resources;
            assert!(
                !view.mutations.first().expect("completed action").pending,
                "the final notification cannot leave the operation stuck"
            );
            assert!(
                view.packages.first().expect("package").can_install,
                "confirmed completion releases conflicting installation controls"
            );
        }
    }
}

#[test]
fn multiple_notifications_coalesce_without_polling_after_an_ordinary_pending_result() {
    let core = ready();
    let mut pending = start(&core);
    for _ in 0..4 {
        let mut load = request(send(&core, Event::Refresh));
        assert!(
            resource_requests(replace(&core, &mut load)).is_empty(),
            "notifications wait for the outstanding mutation"
        );
    }
    let mut status = one_status(
        core.resolve(&mut pending, Ok(progress(1, ResourceActionStage::Received)))
            .expect("late receipt"),
    );
    let effects = core
        .resolve(
            &mut status,
            Ok(progress(
                2,
                ResourceActionStage::Running("Installing".into()),
            )),
        )
        .expect("latest pending result");
    assert!(
        resource_requests(effects).is_empty(),
        "a consumed invalidation cannot start an automatic polling loop"
    );
}

#[test]
fn queued_discovery_does_not_repeat_a_status_query_started_after_the_notification() {
    for status_first in [false, true] {
        let core = ready();
        let mut pending = start(&core);
        let _effects = core
            .resolve(&mut pending, Ok(progress(1, ResourceActionStage::Received)))
            .expect("receipt");
        let mut load = request(send(&core, Event::Refresh));
        let _effects = send(&core, Event::Refresh);
        let mut status = None;
        let mut follow = None;
        for operation in resource_requests(replace(&core, &mut load)) {
            if operation.operation.kind == ResourceOperationKind::Status(identity()) {
                status = Some(operation);
            } else {
                assert_eq!(
                    operation.operation.kind,
                    ResourceOperationKind::Snapshot,
                    "queued discovery"
                );
                follow = Some(operation);
            }
        }
        let mut status = status.expect("status query after both notifications");
        let mut follow = follow.expect("queued snapshot");
        let result = Ok(progress(
            2,
            ResourceActionStage::Running("Installing".into()),
        ));
        let effects = if status_first {
            let mut effects = core.resolve(&mut status, result).expect("fresh status");
            effects.extend(replace(&core, &mut follow));
            effects
        } else {
            let mut effects = replace(&core, &mut follow);
            effects.extend(core.resolve(&mut status, result).expect("fresh status"));
            effects
        };
        assert!(
            resource_requests(effects).is_empty(),
            "queued discovery cannot invent a newer action invalidation"
        );
    }
}

#[test]
fn an_interrupted_request_still_drains_its_queued_notification_once() {
    let core = ready();
    let mut pending = start(&core);
    let mut load = request(send(&core, Event::Refresh));
    let _effects = replace(&core, &mut load);
    let mut status = one_status(
        core.resolve(
            &mut pending,
            Err(ResourceError {
                code: ResourceErrorCode::Unavailable,
                message: "Response lost".into(),
                retry: ResourceRetryAdvice::Never,
            }),
        )
        .expect("lost action response"),
    );
    assert!(
        resource_requests(
            core.resolve(&mut status, Ok(ResourceResult::Unknown))
                .expect("unknown retained outcome")
        )
        .is_empty(),
        "unknown status preserves uncertainty without polling or execution retry"
    );
    assert!(
        core.view()
            .resources
            .mutations
            .first()
            .expect("action")
            .pending,
        "transport errors cannot establish completion"
    );
}

#[test]
fn terminal_results_do_not_schedule_a_queued_status_check() {
    for stage in [
        ResourceActionStage::Succeeded,
        ResourceActionStage::Failed(ResourceError {
            code: ResourceErrorCode::Unavailable,
            message: "Installation failed".into(),
            retry: ResourceRetryAdvice::Never,
        }),
    ] {
        let core = ready();
        let mut pending = start(&core);
        let mut load = request(send(&core, Event::Refresh));
        let effects = core
            .resolve(&mut pending, Ok(progress(1, stage)))
            .expect("terminal runtime result");
        assert!(
            effects
                .iter()
                .any(|effect| matches!(effect, Effect::Render(_))),
            "terminal progress renders even while a discovery read is pending"
        );
        assert!(
            resource_requests(effects).is_empty(),
            "terminal results retire queued status checks"
        );
        let mut requests = resource_requests(replace(&core, &mut load));
        if let Some(mut follow) = requests.pop() {
            assert_eq!(
                follow.operation.kind,
                ResourceOperationKind::Snapshot,
                "success may request newer discovery but never another status"
            );
            assert!(
                resource_requests(replace(&core, &mut follow)).is_empty(),
                "terminal actions stay terminal after discovery"
            );
        }
        assert!(requests.is_empty(), "no extra recovery requests");
    }
}

#[test]
fn reconnect_retires_old_notifications_and_responses_before_status_recovery() {
    let core = ready();
    let mut pending = start(&core);
    let mut load = request(send(&core, Event::Refresh));
    let _effects = replace(&core, &mut load);
    let _effects = send(&core, Event::Suspend);
    let mut load = request(send(&core, Event::Reconnect));
    let mut status = one_status(replace(&core, &mut load));
    assert!(
        resource_requests(
            core.resolve(
                &mut pending,
                Ok(progress(1, ResourceActionStage::Succeeded))
            )
            .expect("retired action response")
        )
        .is_empty(),
        "a retired response cannot drive the new connection"
    );
    assert!(
        resource_requests(
            core.resolve(&mut status, Ok(progress(2, ResourceActionStage::Received)))
                .expect("fresh status")
        )
        .is_empty(),
        "notifications from before reconnect cannot cause extra polling"
    );
    assert!(
        core.view()
            .resources
            .mutations
            .first()
            .expect("action")
            .pending,
        "only the fresh authenticated status determines the outcome"
    );
}
