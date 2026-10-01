use crux_core::Request;

use super::{context, ready, reference, request, send, snapshot};
use crate::{
    Core, Effect, Event as RootEvent, history,
    projections::{Event, ProjectionKind, ProjectionLoadState, ProjectionSelection},
    subscriptions::{self, SubscriptionOperation, SubscriptionResult},
    workspace::{
        self, RepositoryInfo, WorkspaceInfo, WorkspaceMode, WorkspaceOperation, WorkspaceResult,
    },
};

fn subscription_request(effects: Vec<Effect>) -> Request<SubscriptionOperation> {
    effects
        .into_iter()
        .find_map(|effect| match effect {
            Effect::Subscription(request) => Some(*request),
            Effect::Render(_)
            | Effect::HostInfo(_)
            | Effect::History(_)
            | Effect::Workspace(_)
            | Effect::Session(_)
            | Effect::Projection(_) => None,
        })
        .expect("subscription request")
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
            | Effect::Projection(_) => None,
        })
        .expect("workspace request")
}

#[test]
fn supplied_references_drill_into_the_bound_chain_and_foreign_references_fail() {
    let core = ready();
    let selection = ProjectionSelection {
        kind: ProjectionKind::Task,
        key: "stable-row".into(),
    };
    let mut foreign = reference();
    foreign.observation = Some("ff".repeat(32));
    let effects = send(
        &core,
        Event::Inspect {
            selection: selection.clone(),
            reference: foreign,
        },
    );
    assert!(
        effects
            .iter()
            .all(|effect| matches!(effect, Effect::Render(_))),
        "foreign reference cannot trigger history work"
    );
    let effects = send(
        &core,
        Event::Inspect {
            selection,
            reference: reference(),
        },
    );
    assert!(effects.iter().any(|effect| matches!(effect, Effect::History(request) if request.operation.chain == "chain" && matches!(&request.operation.action, history::QueryAction::OperationDetails { operation } if Some(operation) == reference().observation.as_ref()))), "drill-down queries the exact full observation in its chain");
    assert_eq!(
        core.view().history.selected.item,
        reference().item,
        "logical selection remains separate"
    );
    assert_eq!(
        core.view().history.selected.observation,
        reference().observation,
        "physical selection survives"
    );
}

#[test]
fn subscription_changes_refresh_projections_and_audience_changes_clear_them() {
    let core = ready();
    let mut join = subscription_request(core.process_event(RootEvent::Subscriptions(
        subscriptions::Event::Connect(context()),
    )));
    let effects = core
        .resolve(
            &mut join,
            Ok(SubscriptionResult::Joined {
                connection: "connection".into(),
            }),
        )
        .expect("joined");
    let mut watch = None;
    let mut projection = None;
    for effect in effects {
        match effect {
            Effect::Subscription(request) => watch = Some(*request),
            Effect::Projection(request) => projection = Some(*request),
            Effect::Render(_)
            | Effect::HostInfo(_)
            | Effect::History(_)
            | Effect::Workspace(_)
            | Effect::Session(_) => {}
        }
    }
    let mut projection = projection.expect("fresh read after join");
    let mut watch = watch.expect("buffered watch");
    let mut refreshed = request(
        core.resolve(&mut watch, Ok(SubscriptionResult::Changed))
            .expect("change during read"),
    );
    let _effects = core
        .resolve(&mut projection, Ok(snapshot()))
        .expect("retired read");
    assert_eq!(
        core.view().projections.load,
        ProjectionLoadState::Loading,
        "change retires an in-flight projection result"
    );
    let _effects = core
        .resolve(&mut refreshed, Ok(snapshot()))
        .expect("new read");
    let mut other = context();
    other.contributor = "bob".into();
    let _effects = core.process_event(RootEvent::Subscriptions(subscriptions::Event::Connect(
        other,
    )));
    assert!(
        core.view().projections.context.is_none(),
        "audience change retires projection scope"
    );
    assert!(
        core.view().projections.tasks.rows.is_empty(),
        "private rows are not retained for another audience"
    );
    assert!(
        send(&core, Event::Connect(context())).is_empty(),
        "projection context cannot override active audience"
    );
}

#[test]
fn workspace_switch_clears_projections_while_repository_selection_preserves_them() {
    for mode in [WorkspaceMode::Standalone, WorkspaceMode::Managed] {
        let core = Core::new();
        let workspace = WorkspaceInfo {
            id: "workspace".into(),
            name: "Workspace".into(),
            chain: "chain".into(),
            revision: 1,
            mode,
            repositories: vec![RepositoryInfo {
                id: "repo".into(),
                name: "Repo".into(),
                remote: None,
            }],
        };
        let mut other = workspace.clone();
        other.id = "other".into();
        other.chain = "other-chain".into();
        let mut load =
            workspace_request(core.process_event(RootEvent::Workspace(workspace::Event::Load)));
        let _effects = core
            .resolve(
                &mut load,
                Ok(WorkspaceResult::Directory(vec![workspace, other])),
            )
            .expect("directory");
        let _effects = core.process_event(RootEvent::Workspace(workspace::Event::SelectWorkspace(
            "workspace".into(),
        )));
        let mut load = request(send(&core, Event::Connect(context())));
        let _effects = core
            .resolve(&mut load, Ok(snapshot()))
            .expect("projection inputs");
        let before = core.view().projections;
        let _effects = core.process_event(RootEvent::Workspace(
            workspace::Event::SelectRepository(Some("repo".into())),
        ));
        assert_eq!(
            core.view().projections,
            before,
            "repository navigation preserves workspace projection scope"
        );
        let mut old = request(send(&core, Event::Refresh));
        let _effects = core.process_event(RootEvent::Workspace(workspace::Event::SelectWorkspace(
            "other".into(),
        )));
        let _effects = core
            .resolve(&mut old, Ok(snapshot()))
            .expect("retired old workspace reply");
        assert!(
            core.view().projections.context.is_none(),
            "workspace selection retires results"
        );
        assert!(
            send(&core, Event::Connect(context())).is_empty(),
            "cannot reconnect a mismatched workspace"
        );
    }
}

#[test]
fn connecting_after_subscription_start_waits_for_notification_buffering() {
    let core = Core::new();
    let mut join = subscription_request(core.process_event(RootEvent::Subscriptions(
        subscriptions::Event::Connect(context()),
    )));
    let effects = send(&core, Event::Connect(context()));
    assert!(
        effects
            .iter()
            .all(|effect| matches!(effect, Effect::Render(_))),
        "no projection snapshot before buffered notifications"
    );
    let effects = send(&core, Event::Reconnect);
    assert!(
        effects
            .iter()
            .all(|effect| matches!(effect, Effect::Render(_))),
        "client reconnect cannot bypass the pending join"
    );
    let mut load = request(
        core.resolve(
            &mut join,
            Ok(SubscriptionResult::Joined {
                connection: "joined".into(),
            }),
        )
        .expect("join"),
    );
    let _effects = core
        .resolve(&mut load, Ok(snapshot()))
        .expect("first buffered snapshot");
    assert_eq!(
        core.view().projections.load,
        ProjectionLoadState::Ready,
        "join starts the pending projection snapshot"
    );
}
