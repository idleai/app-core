use super::{directory, failure, info, one, repo, requests, select, send, snapshot};
use crate::history::{HistoryPage, QueryResult};
use crate::workspace::{
    Event, NavigationSection, WorkspaceError, WorkspaceErrorKind, WorkspaceMode,
    WorkspaceOperation, WorkspaceRequestState, WorkspaceResult,
};
use crate::{Core, Effect, Event as RootEvent};

#[test]
fn directory_loads_retry_and_empty_success_are_distinct() {
    let core = Core::new();
    assert_eq!(
        core.view().workspace.directory_state,
        WorkspaceRequestState::Idle,
        "initial discovery"
    );
    let mut first = one(send(&core, Event::Load));
    assert_eq!(
        core.view().workspace.directory_state,
        WorkspaceRequestState::Loading,
        "load is pending"
    );
    assert!(
        requests(send(&core, Event::Load)).is_empty(),
        "duplicate load is coalesced"
    );
    let error = failure(WorkspaceErrorKind::Unavailable);
    let _effects = core
        .resolve(&mut first, Err(error.clone()))
        .expect("provider failure");
    assert_eq!(
        core.view().workspace.directory_state,
        WorkspaceRequestState::Failed(error),
        "failure is presentable"
    );
    let _effects = directory(&core, Vec::new());
    assert_eq!(
        core.view().workspace.directory_state,
        WorkspaceRequestState::Ready,
        "empty success is ready"
    );
    assert!(
        core.view().workspace.workspaces.is_empty(),
        "no workspaces are invented"
    );
}

#[test]
fn standalone_selection_sends_only_a_chain_to_history_without_sign_in() {
    let core = Core::new();
    let standalone = info("local", WorkspaceMode::Standalone);
    let _effects = directory(&core, vec![standalone.clone()]);
    let effects = send(&core, Event::SelectWorkspace("local".into()));
    let mut history = None;
    let mut metadata = None;
    let mut saw_host_info = false;
    for effect in effects {
        match effect {
            Effect::History(request) => history = Some(request),
            Effect::Workspace(request) => metadata = Some(request),
            Effect::Render(_) | Effect::Subscription(_) | Effect::Session(_) => {}
            Effect::HostInfo(_) => saw_host_info = true,
        }
    }
    assert!(
        !saw_host_info,
        "workspace selection needs no host handshake"
    );
    let mut history = history.expect("engine operation");
    assert_eq!(
        history.operation.chain, "chain-local",
        "engine receives the logical reference"
    );
    let mut metadata = metadata.expect("coordination operation");
    assert_eq!(
        metadata.operation,
        WorkspaceOperation::Snapshot {
            workspace_id: "local".into(),
            mode: WorkspaceMode::Standalone
        },
        "local adapter route"
    );
    let _effects = core
        .resolve(
            history.as_mut(),
            Ok(QueryResult::History(HistoryPage::default())),
        )
        .expect("engine page");
    let _presence = one(core
        .resolve(
            metadata.as_mut(),
            Ok(WorkspaceResult::Snapshot(snapshot(standalone, "alice"))),
        )
        .expect("local metadata"));
    let view = core.view();
    assert_eq!(
        view.workspace.selected_repository.as_deref(),
        Some("shared-repo"),
        "single standalone context"
    );
    let binding = view
        .workspace
        .repository_binding
        .expect("explicit repository binding");
    assert_eq!(
        (
            binding.workspace_id.as_str(),
            binding.repository_id.as_str(),
            binding.chain.as_str()
        ),
        ("local", "shared-repo", "chain-local"),
        "binding has workspace scope"
    );
    assert_eq!(
        view.history.chain, view.workspace.chain,
        "root wires the same chain"
    );
    let _effects = send(&core, Event::SelectRepository(None));
    assert!(
        core.view().workspace.selection_error.is_some(),
        "standalone cannot discard its repository context"
    );
    let _effects = core.process_event(RootEvent::History(crate::history::Event::Connect(
        "unrelated".into(),
    )));
    assert_eq!(
        core.view().history.chain.as_deref(),
        Some("chain-local"),
        "direct history actions cannot replace the selected binding"
    );
}

#[test]
fn managed_navigation_keeps_repository_bindings_scoped_and_resources_reusable() {
    let core = Core::new();
    let mut a = info("a", WorkspaceMode::Managed);
    a.repositories.push(repo("second"));
    let b = info("b", WorkspaceMode::Managed);
    let _effects = directory(&core, vec![a.clone(), b.clone()]);
    let _presence = select(&core, &a, "alice");
    assert_eq!(
        core.view().workspace.selected_repository,
        None,
        "multi-repository workspace starts at workspace scope"
    );
    for repo in [Some("shared-repo".into()), Some("second".into()), None] {
        let effects = send(&core, Event::SelectRepository(repo.clone()));
        assert!(
            effects
                .iter()
                .all(|effect| matches!(effect, Effect::Render(_))),
            "repository navigation does not reconnect the same chain"
        );
        assert_eq!(
            core.view().workspace.selected_repository,
            repo,
            "repository navigation"
        );
        assert_eq!(
            core.view().history.chain.as_deref(),
            Some("chain-a"),
            "one chain across repositories"
        );
    }
    let _effects = send(&core, Event::Navigate(NavigationSection::AgentRules));
    let _effects = send(&core, Event::SelectRepository(Some("foreign".into())));
    assert_eq!(
        core.view().workspace.section,
        NavigationSection::AgentRules,
        "invalid selection retains navigation"
    );
    let _presence = select(&core, &b, "bob");
    let view = core.view().workspace;
    assert_eq!(
        view.section,
        NavigationSection::Workspace,
        "workspace switch resets destination"
    );
    assert_eq!(
        view.members.first().expect("member").member.contributor_id,
        "bob",
        "members are scoped"
    );
    assert_eq!(
        view.repository_binding.expect("selected binding").chain,
        "chain-b",
        "shared repository resolves through selected workspace"
    );
    assert_eq!(
        view.repository_bindings
            .iter()
            .filter(|binding| binding.repository_id == "shared-repo")
            .count(),
        2,
        "repository can participate in multiple workspaces"
    );
    let data = view.snapshot.expect("workspace resources");
    assert_eq!(
        data.host_ids,
        vec!["shared-host"],
        "host can be bound to both workspaces"
    );
    assert_eq!(
        data.provider_ids,
        vec!["shared-provider"],
        "provider can be bound to both workspaces"
    );
}

#[test]
fn switching_a_b_a_ignores_old_metadata_and_failures() {
    let core = Core::new();
    let a = info("a", WorkspaceMode::Managed);
    let b = info("b", WorkspaceMode::Managed);
    let _effects = directory(&core, vec![a.clone(), b]);
    let mut old_a = one(send(&core, Event::SelectWorkspace("a".into())));
    let mut old_b = one(send(&core, Event::SelectWorkspace("b".into())));
    let _presence = select(&core, &a, "new");
    let before = core.view();
    assert!(
        core.resolve(
            &mut old_a,
            Ok(WorkspaceResult::Snapshot(snapshot(a, "old")))
        )
        .expect("late A response")
        .is_empty(),
        "old A cannot replace fresh A"
    );
    assert!(
        core.resolve(&mut old_b, Err(failure(WorkspaceErrorKind::Forbidden)))
            .expect("late B failure")
            .is_empty(),
        "old access failure cannot clear A"
    );
    assert_eq!(
        core.view(),
        before,
        "late responses leave current context intact"
    );
}

#[test]
fn directory_removal_clears_selection_and_discards_pending_results() {
    let core = Core::new();
    let a = info("a", WorkspaceMode::Managed);
    let _effects = directory(&core, vec![a.clone()]);
    let mut pending = one(send(&core, Event::SelectWorkspace("a".into())));
    let _effects = directory(&core, Vec::new());
    assert_eq!(
        core.view().workspace.selected_workspace,
        None,
        "removed workspace cannot remain selected"
    );
    assert_eq!(core.view().history.chain, None, "engine detached");
    let _effects = core
        .resolve(
            &mut pending,
            Ok(WorkspaceResult::Snapshot(snapshot(a, "alice"))),
        )
        .expect("removed workspace result");
    assert_eq!(
        core.view().workspace.snapshot,
        None,
        "late result cannot resurrect removed metadata"
    );
}

#[test]
fn adoption_preserves_chain_repository_and_navigation_while_updating_route() {
    let core = Core::new();
    let standalone = info("local", WorkspaceMode::Standalone);
    let _effects = directory(&core, vec![standalone.clone()]);
    let mut old_presence = select(&core, &standalone, "alice");
    let _effects = send(&core, Event::Navigate(NavigationSection::Sessions));
    let mut managed = standalone;
    managed.mode = WorkspaceMode::Managed;
    managed.revision = 2;
    managed.repositories.push(repo("second"));
    let mut refresh = one(directory(&core, vec![managed.clone()]));
    assert_eq!(
        refresh.operation,
        WorkspaceOperation::Snapshot {
            workspace_id: "local".into(),
            mode: WorkspaceMode::Managed
        },
        "adopted route"
    );
    assert_eq!(
        core.view().history.chain.as_deref(),
        Some("chain-local"),
        "adoption preserves logical chain"
    );
    let _presence = one(core
        .resolve(
            &mut refresh,
            Ok(WorkspaceResult::Snapshot(snapshot(managed, "alice"))),
        )
        .expect("managed snapshot"));
    let before = core.view();
    let _effects = core
        .resolve(
            &mut old_presence,
            Err(failure(WorkspaceErrorKind::Unavailable)),
        )
        .expect("old provider result");
    assert_eq!(core.view(), before, "old provider result is ignored");
    assert_eq!(
        before.workspace.selected_repository.as_deref(),
        Some("shared-repo"),
        "repository identity preserved"
    );
    assert_eq!(
        before.workspace.section,
        NavigationSection::Sessions,
        "navigation preserved on adoption"
    );
}

#[test]
fn detaching_repository_and_resources_changes_only_current_workspace() {
    let core = Core::new();
    let a = info("a", WorkspaceMode::Managed);
    let b = info("b", WorkspaceMode::Managed);
    let _effects = directory(&core, vec![a.clone(), b.clone()]);
    let _presence = select(&core, &a, "alice");
    let mut refresh = one(send(&core, Event::RefreshWorkspace));
    let mut detached = snapshot(a, "alice");
    detached.workspace.revision = 2;
    detached.workspace.repositories.clear();
    detached.host_ids.clear();
    detached.provider_ids.clear();
    let _presence = one(core
        .resolve(&mut refresh, Ok(WorkspaceResult::Snapshot(detached)))
        .expect("detach snapshot"));
    assert_eq!(
        core.view().workspace.selected_repository,
        None,
        "detached repository cleared"
    );
    assert_eq!(
        core.view().history.chain.as_deref(),
        Some("chain-a"),
        "empty managed workspace keeps its chain"
    );
    let _presence = select(&core, &b, "bob");
    let remaining = core.view().workspace.snapshot.expect("other workspace");
    assert_eq!(
        remaining.workspace.repositories.len(),
        1,
        "shared repository still attached elsewhere"
    );
    assert_eq!(
        remaining.host_ids,
        ["shared-host"],
        "host is not globally deleted"
    );
    assert_eq!(
        remaining.provider_ids,
        ["shared-provider"],
        "provider is not globally deleted"
    );
}

#[test]
fn transient_failure_retains_stale_data_but_revocation_clears_context() {
    let core = Core::new();
    let a = info("a", WorkspaceMode::Managed);
    let _effects = directory(&core, vec![a.clone()]);
    let _presence = select(&core, &a, "alice");
    let mut refresh = one(send(&core, Event::RefreshWorkspace));
    let error = failure(WorkspaceErrorKind::Unavailable);
    let _effects = core
        .resolve(&mut refresh, Err(error.clone()))
        .expect("offline");
    assert!(
        core.view().workspace.snapshot.is_some(),
        "known metadata retained with failure status"
    );
    assert_eq!(
        core.view().workspace.snapshot_state,
        WorkspaceRequestState::Failed(error),
        "cached data is explicitly stale"
    );
    let mut listing = one(send(&core, Event::Load));
    let mut retry = one(send(&core, Event::RefreshWorkspace));
    let _effects = core
        .resolve(&mut retry, Err(failure(WorkspaceErrorKind::Forbidden)))
        .expect("revoked");
    assert!(
        core.view().workspace.members.is_empty(),
        "revoked metadata hidden"
    );
    assert_eq!(
        core.view().history.chain,
        None,
        "revocation detaches history"
    );
    let _effects = core
        .resolve(&mut listing, Ok(WorkspaceResult::Directory(vec![a])))
        .expect("late pre-revocation directory");
    assert!(
        core.view().workspace.workspaces.is_empty(),
        "old directory cannot resurrect revoked workspace"
    );
}

#[test]
fn bad_directory_is_atomic_and_chain_binding_survives_disconnect() {
    let core = Core::new();
    let a = info("a", WorkspaceMode::Managed);
    let _effects = directory(&core, vec![a.clone()]);
    let mut bad = a.clone();
    bad.revision = 2;
    bad.chain = "wrong-chain".into();
    let _effects = directory(&core, vec![info("tentative", WorkspaceMode::Managed), bad]);
    assert_eq!(
        core.view().workspace.workspaces,
        vec![a.clone()],
        "invalid list cannot partially replace directory"
    );
    assert!(
        matches!(
            core.view().workspace.directory_state,
            WorkspaceRequestState::Failed(WorkspaceError {
                kind: WorkspaceErrorKind::InvalidData,
                ..
            })
        ),
        "chain reassignment rejected"
    );
    let mut tentative = info("tentative", WorkspaceMode::Managed);
    tentative.chain = "correct-chain".into();
    let _effects = directory(&core, vec![a.clone(), tentative]);
    assert_eq!(
        core.view().workspace.directory_state,
        WorkspaceRequestState::Ready,
        "failed list did not poison bindings"
    );
    let _effects = send(&core, Event::Disconnect);
    let mut rebound = a;
    rebound.chain = "new-chain".into();
    rebound.revision = 3;
    let _effects = directory(&core, vec![rebound]);
    assert!(
        core.view().workspace.workspaces.is_empty(),
        "disconnect does not authorize rebinding a known workspace"
    );
}

#[test]
fn malformed_cardinality_duplicates_and_conflicting_revisions_are_rejected() {
    let mut invalid = Vec::new();
    let mut no_repo = info("local", WorkspaceMode::Standalone);
    no_repo.repositories.clear();
    invalid.push(vec![no_repo]);
    let mut many = info("local", WorkspaceMode::Standalone);
    many.repositories.push(repo("second"));
    invalid.push(vec![many]);
    let mut duplicates = info("a", WorkspaceMode::Managed);
    duplicates.repositories.push(repo("shared-repo"));
    invalid.push(vec![duplicates]);
    let a = info("a", WorkspaceMode::Managed);
    invalid.push(vec![a.clone(), a.clone()]);
    let mut chain_shared = info("b", WorkspaceMode::Managed);
    chain_shared.chain.clone_from(&a.chain);
    invalid.push(vec![a.clone(), chain_shared]);
    let mut empty = a.clone();
    empty.chain.clear();
    invalid.push(vec![empty]);
    for list in invalid {
        let core = Core::new();
        let _effects = directory(&core, list);
        assert!(
            matches!(
                core.view().workspace.directory_state,
                WorkspaceRequestState::Failed(_)
            ),
            "invalid provider directory rejected"
        );
        assert!(
            core.view().workspace.workspaces.is_empty(),
            "no invalid bindings exposed"
        );
    }
    let core = Core::new();
    let _effects = directory(&core, vec![a.clone()]);
    let mut conflict = a.clone();
    conflict.name = "Changed without a revision".into();
    let _effects = directory(&core, vec![conflict]);
    assert_eq!(
        core.view().workspace.workspaces,
        [a],
        "same revision conflict cannot replace facts"
    );
}

#[test]
fn wrong_result_kind_scope_and_snapshot_chain_cannot_change_context() {
    let core = Core::new();
    let a = info("a", WorkspaceMode::Managed);
    let _effects = directory(&core, vec![a.clone()]);
    let mut pending = one(send(&core, Event::SelectWorkspace("a".into())));
    let _effects = core
        .resolve(&mut pending, Ok(WorkspaceResult::Directory(Vec::new())))
        .expect("wrong result kind");
    assert!(
        matches!(
            core.view().workspace.snapshot_state,
            WorkspaceRequestState::Failed(_)
        ),
        "wrong operation response rejected"
    );
    let mut pending = one(send(&core, Event::RefreshWorkspace));
    let _effects = core
        .resolve(
            &mut pending,
            Ok(WorkspaceResult::Snapshot(snapshot(
                info("b", WorkspaceMode::Managed),
                "bob",
            ))),
        )
        .expect("wrong scope");
    assert_eq!(
        core.view().workspace.snapshot,
        None,
        "foreign members never exposed"
    );
    let mut bad = snapshot(a, "alice");
    bad.workspace.chain = "different-chain".into();
    bad.workspace.revision = 2;
    let mut pending = one(send(&core, Event::RefreshWorkspace));
    let _effects = core
        .resolve(&mut pending, Ok(WorkspaceResult::Snapshot(bad)))
        .expect("wrong chain");
    assert_eq!(
        core.view().history.chain.as_deref(),
        Some("chain-a"),
        "invalid metadata cannot reroute engine"
    );
}

#[test]
fn removing_workspace_discards_inflight_history_and_independent_clients_stay_isolated() {
    let core = Core::new();
    let other = Core::new();
    let _effects = directory(&core, vec![info("a", WorkspaceMode::Managed)]);
    let effects = send(&core, Event::SelectWorkspace("a".into()));
    let mut query = effects
        .into_iter()
        .find_map(|effect| match effect {
            Effect::History(request) => Some(request),
            Effect::Workspace(_)
            | Effect::Render(_)
            | Effect::HostInfo(_)
            | Effect::Subscription(_)
            | Effect::Session(_) => None,
        })
        .expect("history query");
    let _effects = send(&core, Event::Disconnect);
    let _effects = core
        .resolve(
            query.as_mut(),
            Ok(QueryResult::History(HistoryPage::default())),
        )
        .expect("late engine response");
    assert_eq!(
        core.view().history.chain,
        None,
        "late history never restores disconnected context"
    );
    assert_eq!(
        core.view().workspace,
        other.view().workspace,
        "disconnected and untouched clients have independent empty views"
    );
}
