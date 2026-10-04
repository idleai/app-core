use crux_core::Request;

use super::{
    changes, connected, mutation_id, received, request, requests, select, send, snapshot, submit,
};
use crate::{
    Core, Effect, Event as RootEvent,
    sessions::{Event, SessionContext, SessionOperation, SessionResult},
    subscriptions,
    workspace::{
        self, RepositoryInfo, WorkspaceInfo, WorkspaceMode, WorkspaceOperation, WorkspaceResult,
    },
};

fn subscription_context(context: &SessionContext) -> subscriptions::Context {
    subscriptions::Context {
        provider: context.provider.clone(),
        workspace: context.workspace_id.clone(),
        contributor: context.contributor_id.clone(),
        chain: context.chain.clone(),
    }
}

fn subscription_request(effects: Vec<Effect>) -> Request<subscriptions::SubscriptionOperation> {
    effects
        .into_iter()
        .find_map(|effect| {
            if let Effect::Subscription(request) = effect {
                Some(*request)
            } else {
                None
            }
        })
        .expect("subscription request")
}

fn subscribe(
    core: &Core,
    context: &SessionContext,
) -> Request<subscriptions::SubscriptionOperation> {
    let mut join = subscription_request(core.process_event(RootEvent::Subscriptions(
        subscriptions::Event::Connect(subscription_context(context)),
    )));
    subscription_request(
        core.resolve(
            &mut join,
            Ok(subscriptions::SubscriptionResult::Joined {
                connection: "joined".into(),
            }),
        )
        .expect("subscription joined"),
    )
}

fn assert_retired(
    core: &Core,
    watch: &mut Request<SessionOperation>,
    prompt: &mut Request<SessionOperation>,
) {
    let retired = core.view().sessions;
    assert!(retired.context.is_none(), "session context is retired");
    assert!(retired.sessions.is_empty(), "session rows are cleared");
    assert!(retired.prompts.is_empty(), "private prompts are cleared");
    assert!(retired.mutations.is_empty(), "pending actions are cleared");
    assert!(retired.selected.is_none(), "session selection is cleared");
    let result = received(&prompt.operation);
    assert!(
        requests(
            core.resolve(prompt, Ok(result))
                .expect("late prompt result")
        )
        .is_empty(),
        "retired prompt cannot start further requests"
    );
    let result = changes(watch, vec![]);
    assert!(
        requests(core.resolve(watch, Ok(result)).expect("late session watch")).is_empty(),
        "retired watch cannot resubscribe"
    );
    assert_eq!(
        core.view().sessions,
        retired,
        "late results cannot restore retired session state"
    );
    assert!(
        requests(send(core, Event::Refresh)).is_empty(),
        "refresh requires a new session context"
    );
    assert!(
        requests(send(
            core,
            Event::Submit {
                id: mutation_id("after-retirement"),
                text: "new prompt".into(),
            }
        ))
        .is_empty(),
        "retired sessions cannot submit new prompts"
    );
}

#[test]
fn unauthorized_subscription_watch_retires_sessions_and_late_results() {
    for mode in [WorkspaceMode::Standalone, WorkspaceMode::Managed] {
        let source = snapshot(mode, "contributor-bob");
        let (core, mut session_watch) = connected(source.clone());
        select(&core);
        let mut prompt = submit(&core, "private-before-expiry");
        let mut watch = subscribe(&core, &source.context);
        let _effects = core
            .resolve(
                &mut watch,
                Err(subscriptions::SubscriptionError {
                    kind: subscriptions::SubscriptionErrorKind::Unauthorized,
                    message: "Workspace access expired".into(),
                }),
            )
            .expect("expired subscription");
        assert_eq!(
            core.view().subscriptions.status,
            subscriptions::ConnectionStatus::Expired,
            "authorization loss expires the subscription"
        );
        assert_retired(&core, &mut session_watch, &mut prompt);
    }
}

#[test]
fn subscription_disconnect_retires_sessions_and_late_results() {
    let source = snapshot(WorkspaceMode::Managed, "contributor-bob");
    let (core, mut session_watch) = connected(source.clone());
    select(&core);
    let mut prompt = submit(&core, "private-before-disconnect");
    let mut watch = subscribe(&core, &source.context);
    let _effects = core.process_event(RootEvent::Subscriptions(subscriptions::Event::Disconnect));
    let retired = core.view();
    let effects = core
        .resolve(&mut watch, Ok(subscriptions::SubscriptionResult::Changed))
        .expect("late subscription watch");
    assert!(
        effects.is_empty(),
        "retired subscription cannot restart work"
    );
    assert_eq!(core.view(), retired, "late invalidation changes no state");
    assert_retired(&core, &mut session_watch, &mut prompt);
}

#[test]
fn unauthorized_join_retires_pending_session_snapshot() {
    let source = snapshot(WorkspaceMode::Managed, "contributor-bob");
    let core = Core::new();
    let mut load = request(send(&core, Event::Connect(source.context.clone())));
    let mut join = subscription_request(core.process_event(RootEvent::Subscriptions(
        subscriptions::Event::Connect(subscription_context(&source.context)),
    )));
    let _effects = core
        .resolve(
            &mut join,
            Err(subscriptions::SubscriptionError {
                kind: subscriptions::SubscriptionErrorKind::Unauthorized,
                message: "Workspace access denied".into(),
            }),
        )
        .expect("unauthorized join");
    let retired = core.view();
    assert!(
        retired.sessions.context.is_none(),
        "session load is retired"
    );
    assert!(
        requests(
            core.resolve(&mut load, Ok(SessionResult::Snapshot(Box::new(source))))
                .expect("late session snapshot")
        )
        .is_empty(),
        "retired snapshot cannot start a watch"
    );
    assert_eq!(
        core.view(),
        retired,
        "late snapshot restores no private data"
    );
}

#[test]
fn subscription_transport_loss_preserves_the_session_context_and_prompt() {
    let source = snapshot(WorkspaceMode::Managed, "contributor-bob");
    let (core, _session_watch) = connected(source.clone());
    select(&core);
    let _prompt = submit(&core, "pending-during-outage");
    let mut watch = subscribe(&core, &source.context);
    let before = core.view().sessions;
    let _effects = core
        .resolve(&mut watch, Ok(subscriptions::SubscriptionResult::Closed))
        .expect("transport interruption");
    assert_eq!(
        core.view().subscriptions.status,
        subscriptions::ConnectionStatus::Waiting,
        "transport interruption waits for retry"
    );
    assert_eq!(
        core.view().sessions,
        before,
        "transport interruption preserves independent session recovery"
    );
}

fn workspace_request(effects: Vec<Effect>) -> Request<WorkspaceOperation> {
    effects
        .into_iter()
        .find_map(|effect| match effect {
            Effect::Workspace(request) => Some(*request),
            Effect::Render(_)
            | Effect::HostInfo(_)
            | Effect::History(_)
            | Effect::Subscription(_)
            | Effect::Session(_)
            | Effect::Projection(_)
            | Effect::Resource(_)
            | Effect::Configuration(_)
            | Effect::Repository(_) => None,
        })
        .expect("workspace request")
}

#[test]
fn root_workspace_binding_preserves_repository_navigation_and_retires_session_context_changes() {
    let source = snapshot(WorkspaceMode::Managed, "contributor-bob");
    let core = Core::new();
    let workspace = WorkspaceInfo {
        id: source.context.workspace_id.clone(),
        name: "Workspace".into(),
        chain: source.context.chain.clone(),
        revision: 1,
        mode: WorkspaceMode::Managed,
        repositories: vec![
            RepositoryInfo {
                id: "repo-one".into(),
                name: "One".into(),
                remote: None,
            },
            RepositoryInfo {
                id: "repo-two".into(),
                name: "Two".into(),
                remote: None,
            },
        ],
    };
    let mut other = workspace.clone();
    other.id = "other-workspace".into();
    other.chain = "other-chain".into();
    let mut list =
        workspace_request(core.process_event(RootEvent::Workspace(workspace::Event::Load)));
    let _effects = core
        .resolve(
            &mut list,
            Ok(WorkspaceResult::Directory(vec![
                workspace.clone(),
                other.clone(),
            ])),
        )
        .expect("workspace directory");
    let _effects = core.process_event(RootEvent::Workspace(workspace::Event::SelectWorkspace(
        workspace.id,
    )));
    let mut wrong = source.context.clone();
    wrong.chain = "wrong-chain".into();
    assert!(
        send(&core, Event::Connect(wrong)).is_empty(),
        "session scope cannot override selected workspace chain"
    );
    let mut load = request(send(&core, Event::Connect(source.context.clone())));
    let mut watch = request(
        core.resolve(&mut load, Ok(SessionResult::Snapshot(Box::new(source))))
            .expect("session snapshot"),
    );
    select(&core);
    let mut prompt = submit(&core, "pending");
    let before = core.view().sessions;
    let _effects = core.process_event(RootEvent::Workspace(workspace::Event::SelectRepository(
        Some("repo-two".into()),
    )));
    assert_eq!(
        core.view().sessions,
        before,
        "repository selection does not change logical session context"
    );
    let _effects = core.process_event(RootEvent::Workspace(workspace::Event::SelectWorkspace(
        other.id,
    )));
    assert!(
        core.view().sessions.context.is_none(),
        "workspace switch retires session state"
    );
    let after = core.view();
    let result = received(&prompt.operation);
    let _effects = core
        .resolve(&mut prompt, Ok(result))
        .expect("retired prompt response");
    let result = changes(&watch, vec![]);
    let _effects = core
        .resolve(&mut watch, Ok(result))
        .expect("retired session watch");
    assert_eq!(
        core.view(),
        after,
        "old session results cannot update another workspace"
    );
}

#[test]
fn root_subscription_context_changes_retire_old_session_state() {
    let source = snapshot(WorkspaceMode::Managed, "contributor-bob");
    let active = subscription_context(&source.context);
    for context in [
        subscriptions::Context {
            contributor: "contributor-alice".into(),
            ..active.clone()
        },
        subscriptions::Context {
            provider: "other-provider".into(),
            ..active.clone()
        },
        subscriptions::Context {
            workspace: "other-workspace".into(),
            ..active.clone()
        },
        subscriptions::Context {
            chain: "other-chain".into(),
            ..active
        },
    ] {
        let (core, mut watch) = connected(source.clone());
        select(&core);
        let mut prompt = submit(&core, "private");
        let _effects = core.process_event(RootEvent::Subscriptions(subscriptions::Event::Connect(
            context,
        )));
        assert_retired(&core, &mut watch, &mut prompt);
    }
}
