use crux_core::Request;

use super::{ready, request, send, snapshot};
use crate::{
    Core, Effect, Event as RootEvent,
    resources::{Event, ResourceLoadState, ResourceResult},
    subscriptions::{self, Context, SubscriptionOperation, SubscriptionResult},
    workspace::{
        self, RepositoryInfo, WorkspaceInfo, WorkspaceMode, WorkspaceOperation, WorkspaceResult,
    },
};

fn subscription_context() -> Context {
    let context = snapshot().context;
    Context {
        provider: context.provider,
        workspace: context.workspace_id,
        contributor: context.contributor_id,
        chain: context.chain,
    }
}

fn subscription_request(effects: Vec<Effect>) -> Request<SubscriptionOperation> {
    effects
        .into_iter()
        .find_map(|effect| {
            if let Effect::Subscription(request) = effect {
                Some(*request)
            } else {
                None
            }
        })
        .expect("subscription effect")
}

fn workspace_request(effects: Vec<Effect>) -> Request<WorkspaceOperation> {
    effects
        .into_iter()
        .find_map(|effect| {
            if let Effect::Workspace(request) = effect {
                Some(*request)
            } else {
                None
            }
        })
        .expect("workspace effect")
}

#[test]
fn resource_loading_waits_for_buffered_join_and_cannot_override_the_audience() {
    let core = Core::new();
    let mut join = subscription_request(core.process_event(RootEvent::Subscriptions(
        subscriptions::Event::Connect(subscription_context()),
    )));
    let effects = send(&core, Event::Connect(snapshot().context));
    assert!(
        effects
            .iter()
            .all(|effect| matches!(effect, Effect::Render(_))),
        "resource snapshot waits for buffered notifications"
    );
    assert_eq!(
        core.view().resources.load,
        ResourceLoadState::Suspended,
        "pending join is explicit"
    );
    assert!(
        send(&core, Event::Reconnect)
            .iter()
            .all(|effect| matches!(effect, Effect::Render(_))),
        "direct reconnect cannot bypass pending join"
    );
    let mut load = request(
        core.resolve(
            &mut join,
            Ok(SubscriptionResult::Joined {
                connection: "joined".into(),
            }),
        )
        .expect("join result"),
    );
    let _effects = core
        .resolve(
            &mut load,
            Ok(ResourceResult::Snapshot(Box::new(snapshot()))),
        )
        .expect("buffered resource load");
    assert_eq!(
        core.view().resources.load,
        ResourceLoadState::Ready,
        "join resumes resources"
    );
    let mut other = snapshot().context;
    other.contributor_id = "foreign".into();
    assert!(
        send(&core, Event::Connect(other)).is_empty(),
        "resource audience cannot override the active connection"
    );
}

#[test]
fn subscription_changes_coalesce_refreshes_and_switching_audience_retires_results() {
    let core = ready();
    let mut join = subscription_request(core.process_event(RootEvent::Subscriptions(
        subscriptions::Event::Connect(subscription_context()),
    )));
    assert_eq!(
        core.view().resources.load,
        ResourceLoadState::Suspended,
        "starting buffered connection retires unbuffered reads"
    );
    let effects = core
        .resolve(
            &mut join,
            Ok(SubscriptionResult::Joined {
                connection: "joined".into(),
            }),
        )
        .expect("join result");
    let mut watch = None;
    let mut load = None;
    for effect in effects {
        if let Effect::Subscription(request) = effect {
            watch = Some(*request);
        } else if let Effect::Resource(request) = effect {
            load = Some(*request);
        }
    }
    let mut watch = watch.expect("watch");
    let mut load = load.expect("resource refresh");
    for _ in 0..5 {
        let effects = core
            .resolve(&mut watch, Ok(SubscriptionResult::Changed))
            .expect("resource invalidation");
        assert!(
            effects
                .iter()
                .all(|effect| !matches!(effect, Effect::Resource(_))),
            "one in-flight resource read"
        );
        watch = subscription_request(effects);
    }
    let mut follow = request(
        core.resolve(
            &mut load,
            Ok(ResourceResult::Snapshot(Box::new(snapshot()))),
        )
        .expect("read followed by queued invalidation"),
    );
    assert_eq!(
        core.view().resources.load,
        ResourceLoadState::Loading,
        "queued changes retain loading state"
    );
    let _effects = core
        .resolve(
            &mut follow,
            Ok(ResourceResult::Snapshot(Box::new(snapshot()))),
        )
        .expect("last replacement");
    let mut old = request(send(&core, Event::Refresh));
    let mut other = subscription_context();
    other.contributor = "another-person".into();
    let _effects = core.process_event(RootEvent::Subscriptions(subscriptions::Event::Connect(
        other,
    )));
    let _effects = core
        .resolve(&mut old, Ok(ResourceResult::Snapshot(Box::new(snapshot()))))
        .expect("retired resource result");
    assert!(
        core.view().resources.context.is_none(),
        "audience switch clears resource scope"
    );
    assert!(
        core.view().resources.hosts.is_empty(),
        "previous audience rows cannot return"
    );
}

#[test]
fn workspace_switch_retires_resources_and_repository_navigation_preserves_them() {
    for mode in [WorkspaceMode::Standalone, WorkspaceMode::Managed] {
        let core = Core::new();
        let mut source = snapshot();
        source.context.mode = mode;
        let info = WorkspaceInfo {
            id: source.context.workspace_id.clone(),
            name: "Workspace".into(),
            chain: source.context.chain.clone(),
            revision: 1,
            mode,
            repositories: vec![RepositoryInfo {
                id: "repo".into(),
                name: "Repo".into(),
                remote: None,
            }],
        };
        let mut other = info.clone();
        other.id = "other-workspace".into();
        other.chain = "other-chain".into();
        let mut directory =
            workspace_request(core.process_event(RootEvent::Workspace(workspace::Event::Load)));
        let _effects = core
            .resolve(
                &mut directory,
                Ok(WorkspaceResult::Directory(vec![info, other])),
            )
            .expect("directory");
        let _effects = core.process_event(RootEvent::Workspace(workspace::Event::SelectWorkspace(
            source.context.workspace_id.clone(),
        )));
        let mut load = request(send(&core, Event::Connect(source.context.clone())));
        let _effects = core
            .resolve(
                &mut load,
                Ok(ResourceResult::Snapshot(Box::new(source.clone()))),
            )
            .expect("resource load");
        let before = core.view().resources;
        let _effects = core.process_event(RootEvent::Workspace(
            workspace::Event::SelectRepository(Some("repo".into())),
        ));
        assert_eq!(
            core.view().resources,
            before,
            "repository navigation leaves workspace resource bindings intact"
        );
        let mut wrong_mode = source.context.clone();
        wrong_mode.mode = match mode {
            WorkspaceMode::Standalone => WorkspaceMode::Managed,
            WorkspaceMode::Managed => WorkspaceMode::Standalone,
        };
        assert!(
            send(&core, Event::Connect(wrong_mode)).is_empty(),
            "resource adapter mode must match selected workspace"
        );
        let mut old = request(send(&core, Event::Refresh));
        let _effects = core.process_event(RootEvent::Workspace(workspace::Event::SelectWorkspace(
            "other-workspace".into(),
        )));
        let _effects = core
            .resolve(
                &mut old,
                Ok(ResourceResult::Snapshot(Box::new(source.clone()))),
            )
            .expect("retired old workspace read");
        assert!(
            core.view().resources.context.is_none(),
            "workspace switch retires resource state"
        );
        assert!(
            send(&core, Event::Connect(source.context)).is_empty(),
            "resource connect cannot override the selected workspace"
        );
    }
}
