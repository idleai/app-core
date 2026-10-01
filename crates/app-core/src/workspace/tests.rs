//! End-to-end reducer tests through actual Crux effect continuations.

mod adapter_cases;
mod presence_cases;
mod state_cases;
mod wire_cases;

use crux_core::Request;

use super::{
    Event, MemberInfo, MemberRole, MemberStatus, PresenceEntry, PresenceStatus, RepositoryInfo,
    WorkspaceError, WorkspaceErrorKind, WorkspaceInfo, WorkspaceMode, WorkspaceOperation,
    WorkspaceResult, WorkspaceSnapshot,
};
use crate::{Core, Effect, Event as RootEvent};

fn info(id: &str, mode: WorkspaceMode) -> WorkspaceInfo {
    WorkspaceInfo {
        id: id.into(),
        name: format!("Workspace {id}"),
        chain: format!("chain-{id}"),
        revision: 1,
        mode,
        repositories: vec![repo("shared-repo")],
    }
}

fn repo(id: &str) -> RepositoryInfo {
    RepositoryInfo {
        id: id.into(),
        name: id.into(),
        remote: None,
    }
}

fn member(id: &str) -> MemberInfo {
    MemberInfo {
        contributor_id: id.into(),
        display_name: format!("Person {id}"),
        revision: 1,
        role: MemberRole::Member,
        status: MemberStatus::Active,
    }
}

fn snapshot(workspace: WorkspaceInfo, id: &str) -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        workspace,
        members: vec![member(id)],
        host_ids: vec!["shared-host".into()],
        provider_ids: vec!["shared-provider".into()],
    }
}

fn presence(id: &str, connection: &str, status: PresenceStatus) -> PresenceEntry {
    PresenceEntry {
        contributor_id: id.into(),
        connection_id: connection.into(),
        status,
        repository_id: Some("shared-repo".into()),
        branch: Some("feature".into()),
        file: Some("src/lib.rs".into()),
        host_id: Some("shared-host".into()),
        summary: Some("Editing workspace state".into()),
        observed_at_ms: 100,
        valid_until_ms: 200,
    }
}

fn failure(kind: WorkspaceErrorKind) -> WorkspaceError {
    WorkspaceError {
        kind,
        message: format!("{kind:?}"),
    }
}

fn send(core: &Core, event: Event) -> Vec<Effect> {
    core.process_event(RootEvent::Workspace(event))
}

fn requests(effects: Vec<Effect>) -> Vec<Request<WorkspaceOperation>> {
    effects
        .into_iter()
        .filter_map(|effect| match effect {
            Effect::Workspace(request) => Some(*request),
            Effect::Render(_)
            | Effect::HostInfo(_)
            | Effect::History(_)
            | Effect::Subscription(_)
            | Effect::Session(_)
            | Effect::Projection(_)
            | Effect::Resource(_) => None,
        })
        .collect()
}

fn one(effects: Vec<Effect>) -> Request<WorkspaceOperation> {
    let mut requests = requests(effects).into_iter();
    let request = requests.next().expect("one workspace request");
    assert!(requests.next().is_none(), "exactly one workspace operation");
    request
}

fn directory(core: &Core, workspaces: Vec<WorkspaceInfo>) -> Vec<Effect> {
    let mut request = one(send(core, Event::Load));
    assert_eq!(
        request.operation,
        WorkspaceOperation::List,
        "host discovers available connections"
    );
    core.resolve(&mut request, Ok(WorkspaceResult::Directory(workspaces)))
        .expect("resolve directory")
}

fn select(core: &Core, info: &WorkspaceInfo, contributor: &str) -> Request<WorkspaceOperation> {
    let mut request = one(send(core, Event::SelectWorkspace(info.id.clone())));
    one(core
        .resolve(
            &mut request,
            Ok(WorkspaceResult::Snapshot(snapshot(
                info.clone(),
                contributor,
            ))),
        )
        .expect("resolve metadata"))
}
