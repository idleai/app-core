use crux_core::Request;

use super::{
    Event, ReadReport, ReadState, RecordedSession, RepositoryAction, RepositoryContext,
    RepositoryLoadState, RepositoryQuery, RepositoryResult, RepositoryScope, RepositorySnapshot,
    SourceRecord,
};
use crate::{Core, Effect, Event as RootEvent, history, subscriptions::Context};

fn context(workspace: &str) -> RepositoryContext {
    RepositoryContext {
        connection: Context {
            provider: "local".into(),
            workspace: workspace.into(),
            chain: format!("chain:{workspace}"),
            contributor: "contributor".into(),
        },
        repository_id: format!("repository:{workspace}"),
    }
}

fn session(value: u8) -> RecordedSession {
    let id = format!("{value:02x}").repeat(32);
    RecordedSession {
        id: id.clone(),
        labels: vec![format!("Session {value}")],
        actions: vec!["Snapshot".into()],
        sources: vec!["local capture".into()],
        records: vec![SourceRecord {
            observation: format!("{:02x}", value.saturating_add(10)).repeat(32),
            item: id,
            record_hash: "ff".repeat(32),
        }],
    }
}

fn snapshot(context: &RepositoryContext) -> RepositorySnapshot {
    RepositorySnapshot {
        scope: RepositoryScope {
            workspace_id: context.connection.workspace.clone(),
            repository_id: context.repository_id.clone(),
            chain: context.connection.chain.clone(),
        },
        checked_at_ms: 100,
        checkout: None,
        github: None,
        account: None,
        git_authors: Vec::new(),
        contributors: Vec::new(),
        collaborators: Vec::new(),
        sessions: vec![session(1), session(2)],
        reports: vec![ReadReport {
            topic: "history.sessions".into(),
            state: ReadState::Complete,
            message: "Recorded session items".into(),
            checked_at_ms: 100,
            retry_at_ms: None,
            source_url: None,
        }],
    }
}

fn request(effects: Vec<Effect>) -> Request<RepositoryQuery> {
    effects
        .into_iter()
        .find_map(|effect| {
            if let Effect::Repository(request) = effect {
                Some(*request)
            } else {
                None
            }
        })
        .expect("repository request")
}

fn send(core: &Core, event: Event) -> Vec<Effect> {
    core.process_event(RootEvent::Repository(event))
}

fn select_workspace(core: &Core, context: &RepositoryContext) {
    use crate::workspace::{self, RepositoryInfo, WorkspaceInfo, WorkspaceMode, WorkspaceResult};

    let mut directory = core
        .process_event(RootEvent::Workspace(workspace::Event::Load))
        .into_iter()
        .find_map(|effect| {
            if let Effect::Workspace(request) = effect {
                Some(*request)
            } else {
                None
            }
        })
        .expect("workspace directory");
    let _effects = core
        .resolve(
            &mut directory,
            Ok(WorkspaceResult::Directory(vec![WorkspaceInfo {
                id: context.connection.workspace.clone(),
                name: "Workspace".into(),
                chain: context.connection.chain.clone(),
                revision: 1,
                mode: WorkspaceMode::Standalone,
                repositories: vec![RepositoryInfo {
                    id: context.repository_id.clone(),
                    name: "Repository".into(),
                    remote: None,
                }],
            }])),
        )
        .expect("loaded directory");
    let _effects = core.process_event(RootEvent::Workspace(workspace::Event::SelectWorkspace(
        context.connection.workspace.clone(),
    )));
}

fn loaded(core: &Core, context: &RepositoryContext) {
    let mut read = request(send(core, Event::Connect(context.clone())));
    let _effects = core
        .resolve(
            &mut read,
            Ok(RepositoryResult::Snapshot {
                snapshot: Box::new(snapshot(context)),
                selected_session: None,
            }),
        )
        .expect("repository replacement");
}

#[test]
fn stale_replies_and_foreign_bindings_cannot_replace_the_selected_repository() {
    let core = Core::new();
    let first = context("one");
    let second = context("two");
    let mut old = request(send(&core, Event::Connect(first.clone())));
    let mut current = request(send(&core, Event::Connect(second.clone())));
    let _effects = core
        .resolve(
            &mut old,
            Ok(RepositoryResult::Snapshot {
                snapshot: Box::new(snapshot(&first)),
                selected_session: None,
            }),
        )
        .expect("retired result");
    assert!(
        core.view().repository.snapshot.is_none(),
        "retired result cannot populate a new context"
    );
    let _effects = core
        .resolve(
            &mut current,
            Ok(RepositoryResult::Snapshot {
                snapshot: Box::new(snapshot(&first)),
                selected_session: None,
            }),
        )
        .expect("foreign result rejected by reducer");
    assert!(
        matches!(core.view().repository.load, RepositoryLoadState::Failed(_)),
        "foreign workspace is a visible failure"
    );
    assert!(
        core.view().repository.snapshot.is_none(),
        "no foreign rows are retained"
    );
    let mut current = request(send(&core, Event::Refresh));
    let _effects = core
        .resolve(
            &mut current,
            Ok(RepositoryResult::Snapshot {
                snapshot: Box::new(snapshot(&second)),
                selected_session: None,
            }),
        )
        .expect("current result");
    assert_eq!(
        core.view()
            .repository
            .snapshot
            .expect("snapshot")
            .scope
            .workspace_id,
        "two",
        "only the current binding is admitted"
    );
}

#[test]
fn selected_session_maps_to_exact_history_and_late_preferences_do_not_replace_it() {
    let core = Core::new();
    let context = context("workspace");
    loaded(&core, &context);
    let first = session(1).id;
    let second = session(2).id;
    let effects = send(&core, Event::SelectSession(Some(first.clone())));
    assert_eq!(
        core.view().history.filter.session,
        Some(first.clone()),
        "history filter uses the full logical session identity"
    );
    assert_eq!(
        core.view().history.chain.as_deref(),
        Some(context.connection.chain.as_str()),
        "selection stays in the repository chain"
    );
    let mut save = request(effects);
    assert_eq!(
        save.operation.action,
        RepositoryAction::Remember(Some(first.clone())),
        "host is asked to retain the exact selection"
    );
    let _effects = core
        .resolve(&mut save, Ok(RepositoryResult::Remembered))
        .expect("preference saved");
    let mut refresh = request(send(&core, Event::Refresh));
    let _effects = send(&core, Event::SelectSession(Some(second.clone())));
    let _effects = core
        .resolve(
            &mut refresh,
            Ok(RepositoryResult::Snapshot {
                snapshot: Box::new(snapshot(&context)),
                selected_session: Some(first),
            }),
        )
        .expect("replacement");
    assert_eq!(
        core.view().repository.selected_session,
        Some(second.clone()),
        "a saved preference cannot replace a newer local choice"
    );
    assert_eq!(
        core.view().history.filter.session,
        Some(second),
        "history follows the newer selection"
    );
}

#[test]
fn reopened_selection_is_restored_and_transient_source_failures_keep_it() {
    let core = Core::new();
    let context = context("workspace");
    let selected = session(1).id;
    let mut read = request(send(&core, Event::Connect(context.clone())));
    let _effects = core
        .resolve(
            &mut read,
            Ok(RepositoryResult::Snapshot {
                snapshot: Box::new(snapshot(&context)),
                selected_session: Some(selected.clone()),
            }),
        )
        .expect("restored selection");
    assert_eq!(
        core.view().repository.selected_session,
        Some(selected.clone()),
        "reopened view restores a session admitted in the current snapshot"
    );
    assert_eq!(
        core.view().history.filter.session,
        Some(selected.clone()),
        "restoration also selects exact session history"
    );
    let mut refresh = request(send(&core, Event::Refresh));
    let mut unavailable = snapshot(&context);
    unavailable.sessions.clear();
    unavailable
        .reports
        .first_mut()
        .expect("session report")
        .state = ReadState::Unavailable;
    let _effects = core
        .resolve(
            &mut refresh,
            Ok(RepositoryResult::Snapshot {
                snapshot: Box::new(unavailable),
                selected_session: None,
            }),
        )
        .expect("partial replacement");
    assert_eq!(
        core.view().repository.selected_session,
        Some(selected),
        "temporary unavailable lists cannot erase a selection"
    );
    let mut refresh = request(send(&core, Event::Refresh));
    let mut removed = snapshot(&context);
    removed.sessions.clear();
    let _effects = core
        .resolve(
            &mut refresh,
            Ok(RepositoryResult::Snapshot {
                snapshot: Box::new(removed),
                selected_session: None,
            }),
        )
        .expect("complete empty replacement");
    assert!(
        core.view().repository.selected_session.is_none(),
        "a complete replacement clears a removed session"
    );
    assert!(
        core.view().history.filter.session.is_none(),
        "removed sessions no longer filter activity"
    );
}

#[test]
fn delayed_session_restoration_preserves_activity_until_sessions_is_opened() {
    use crate::workspace::{Event as WorkspaceEvent, NavigationSection};

    let core = Core::new();
    let context = context("workspace");
    select_workspace(&core, &context);
    let selected = session(1).id;
    let mut read = request(send(&core, Event::Connect(context.clone())));
    let _effects = core.process_event(RootEvent::Workspace(WorkspaceEvent::Navigate(
        NavigationSection::Activity,
    )));
    let _effects = core
        .resolve(
            &mut read,
            Ok(RepositoryResult::Snapshot {
                snapshot: Box::new(snapshot(&context)),
                selected_session: Some(selected.clone()),
            }),
        )
        .expect("late repository reply");
    let view = core.view();
    assert_eq!(
        view.workspace.section,
        NavigationSection::Activity,
        "navigation stays current"
    );
    assert_eq!(
        view.repository.selected_session,
        Some(selected.clone()),
        "retain the session preference"
    );
    assert!(
        view.history.filter.session.is_none(),
        "Activity remains unfiltered"
    );
    assert_eq!(
        view.history.selected,
        history::Selected::default(),
        "late restoration cannot change Activity selection"
    );
    let _effects = core.process_event(RootEvent::Workspace(WorkspaceEvent::Navigate(
        NavigationSection::Sessions,
    )));
    assert_eq!(
        core.view().history.filter.session,
        Some(selected),
        "opening Sessions applies its retained selection"
    );
}

#[test]
fn exact_session_sources_are_checked_before_history_actions() {
    let core = Core::new();
    loaded(&core, &context("workspace"));
    let known = session(1);
    let record = known.records.first().expect("source").clone();
    let mut changed = record.clone();
    changed.record_hash = "00".repeat(32);
    let effects = send(
        &core,
        Event::Inspect {
            session: known.id.clone(),
            record: changed,
        },
    );
    assert!(
        !effects
            .iter()
            .any(|effect| matches!(effect, Effect::History(_))),
        "a forged source emits no history read"
    );
    let _effects = send(
        &core,
        Event::Inspect {
            session: known.id.clone(),
            record: record.clone(),
        },
    );
    assert_eq!(
        core.view().history.selected,
        history::Selected {
            item: Some(known.id),
            observation: Some(record.observation)
        },
        "inspection opens the supplied full observation"
    );
}

#[test]
fn disconnect_retires_repository_data_selection_and_pending_reads() {
    let core = Core::new();
    let context = context("workspace");
    loaded(&core, &context);
    let _effects = send(&core, Event::SelectSession(Some(session(1).id)));
    let mut read = request(send(&core, Event::Refresh));
    let _effects = core.process_event(RootEvent::Subscriptions(
        crate::subscriptions::Event::Disconnect,
    ));
    let _effects = send(&core, Event::Disconnect);
    let _effects = core
        .resolve(
            &mut read,
            Ok(RepositoryResult::Snapshot {
                snapshot: Box::new(snapshot(&context)),
                selected_session: None,
            }),
        )
        .expect("retired response");
    assert!(
        core.view().repository.context.is_none(),
        "disconnect removes the repository context"
    );
    assert!(
        core.view().repository.snapshot.is_none(),
        "disconnect removes repository data"
    );
    assert!(
        core.view().repository.selected_session.is_none(),
        "disconnect removes its recorded selection"
    );
}

#[test]
fn explicit_refresh_is_not_lost_when_history_changes_during_a_pending_read() {
    let core = Core::new();
    let context = context("workspace");
    let mut first = request(send(&core, Event::Connect(context.clone())));
    let _effects = send(&core, Event::Refresh);
    let _effects = send(&core, Event::Changed);
    let followup = core
        .resolve(
            &mut first,
            Ok(RepositoryResult::Snapshot {
                snapshot: Box::new(snapshot(&context)),
                selected_session: None,
            }),
        )
        .expect("initial read");
    assert_eq!(
        request(followup).operation.action,
        RepositoryAction::Read,
        "explicit refresh takes priority over automatic cache reuse"
    );
}

#[test]
fn repository_shell_round_trip_retains_session_sources_and_rejects_forged_completions() {
    use crate::{
        Shell,
        effects::EffectFfi,
        shell::{EffectBatch, ShellFormat},
    };
    use crux_core::bridge::FfiFormat;
    let encode = |value: &RootEvent| {
        let mut bytes = Vec::new();
        ShellFormat::serialize(&mut bytes, value).expect("event bytes");
        bytes
    };
    let shell = Shell::new();
    let context = context("workspace");
    let batch: EffectBatch = ShellFormat::deserialize(
        &shell
            .process_event(&encode(&RootEvent::Repository(Event::Connect(
                context.clone(),
            ))))
            .expect("connect"),
    )
    .expect("effects");
    let request = batch
        .requests
        .iter()
        .find(|request| matches!(&request.effect, EffectFfi::Repository(_)))
        .expect("repository effect");
    let result = RepositoryResult::Snapshot {
        snapshot: Box::new(snapshot(&context)),
        selected_session: Some(session(1).id),
    };
    let mut bytes = Vec::new();
    ShellFormat::serialize(&mut bytes, &super::RepositoryResponse::Ok(result.clone()))
        .expect("generated response");
    let mut typed = Vec::new();
    ShellFormat::serialize(&mut typed, &Ok::<_, crate::module::EffectError>(result))
        .expect("typed response");
    assert_eq!(
        bytes, typed,
        "generated native response matches the Rust operation result"
    );
    assert!(
        shell.handle_response(request.id, &[0]).is_err(),
        "malformed bytes do not consume the request"
    );
    let _effects = shell
        .handle_response(request.id, &bytes)
        .expect("repository response");
    let view: crate::ViewModel =
        ShellFormat::deserialize(&shell.view().expect("view bytes")).expect("view");
    assert_eq!(
        view.repository.selected_session,
        Some(session(1).id),
        "full logical selection round trips through the shell"
    );
    assert_eq!(
        view.repository.snapshot.expect("snapshot").sessions,
        snapshot(&context).sessions,
        "exact session records retain full addresses and hashes"
    );
    assert!(
        serde_json::from_str::<RootEvent>(
            r#"{"Repository":{"Completed":{"token":1,"result":{}}}}"#
        )
        .is_err(),
        "clients cannot inject host completions"
    );
}

#[test]
fn selecting_a_different_repository_on_the_same_chain_retires_the_old_reader() {
    use crate::workspace::{self, RepositoryInfo, WorkspaceInfo, WorkspaceMode, WorkspaceResult};
    let core = Core::new();
    let mut load = core
        .process_event(RootEvent::Workspace(workspace::Event::Load))
        .into_iter()
        .find_map(|effect| {
            if let Effect::Workspace(request) = effect {
                Some(*request)
            } else {
                None
            }
        })
        .expect("directory request");
    let context = context("workspace");
    let info = WorkspaceInfo {
        id: context.connection.workspace.clone(),
        name: "Workspace".into(),
        chain: context.connection.chain.clone(),
        revision: 1,
        mode: WorkspaceMode::Managed,
        repositories: vec![
            RepositoryInfo {
                id: context.repository_id.clone(),
                name: "First".into(),
                remote: None,
            },
            RepositoryInfo {
                id: "other".into(),
                name: "Other".into(),
                remote: None,
            },
        ],
    };
    let _effects = core
        .resolve(&mut load, Ok(WorkspaceResult::Directory(vec![info])))
        .expect("directory");
    let _effects = core.process_event(RootEvent::Workspace(workspace::Event::SelectWorkspace(
        "workspace".into(),
    )));
    let _effects = core.process_event(RootEvent::Workspace(workspace::Event::SelectRepository(
        Some(context.repository_id.clone()),
    )));
    loaded(&core, &context);
    let mut previous = request(send(&core, Event::Refresh));
    let _effects = core.process_event(RootEvent::Workspace(workspace::Event::SelectRepository(
        Some("other".into()),
    )));
    assert!(
        core.view().repository.context.is_none(),
        "repository change retires data even when the chain is unchanged"
    );
    let _effects = core
        .resolve(
            &mut previous,
            Ok(RepositoryResult::Snapshot {
                snapshot: Box::new(snapshot(&context)),
                selected_session: None,
            }),
        )
        .expect("retired read");
    assert!(
        core.view().repository.snapshot.is_none(),
        "old repository response cannot restore its rows"
    );
}
