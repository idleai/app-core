use crux_core::Request;

use super::{changes, connected, received, request, select, send, snapshot, submit};
use crate::{
    Core, Effect, Event as RootEvent,
    sessions::{Event, SessionResult},
    subscriptions,
    workspace::{
        self, RepositoryInfo, WorkspaceInfo, WorkspaceMode, WorkspaceOperation, WorkspaceResult,
    },
};

fn workspace_request(effects: Vec<Effect>) -> Request<WorkspaceOperation> {
    effects
        .into_iter()
        .find_map(|effect| match effect {
            Effect::Workspace(request) => Some(*request),
            Effect::Render(_)
            | Effect::HostInfo(_)
            | Effect::History(_)
            | Effect::Subscription(_)
            | Effect::Session(_) => None,
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
fn root_subscription_audience_switch_retires_old_session_state() {
    let source = snapshot(WorkspaceMode::Managed, "contributor-bob");
    let (core, _watch) = connected(source.clone());
    select(&core);
    let _prompt = submit(&core, "private");
    let context = subscriptions::Context {
        provider: source.context.provider,
        workspace: source.context.workspace_id,
        contributor: "contributor-alice".into(),
        chain: source.context.chain,
    };
    let _effects = core.process_event(RootEvent::Subscriptions(subscriptions::Event::Connect(
        context,
    )));
    assert!(
        core.view().sessions.context.is_none(),
        "subscription identity changes retire session audience"
    );
    assert!(
        core.view().sessions.prompts.is_empty(),
        "new audience sees no prior local input"
    );
}
