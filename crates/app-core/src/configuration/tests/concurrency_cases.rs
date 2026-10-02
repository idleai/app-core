use super::{
    commit, context, edit, editor, ready, record, request, requests, resolve_load, save, send,
};
use crate::{
    Core,
    configuration::{
        ConfigurationAction, ConfigurationDocument as Document,
        ConfigurationEditorAction as Action, ConfigurationError, ConfigurationErrorKind,
        ConfigurationLoadState as LoadState, ConfigurationSaveState as SaveState, Event,
    },
    workspace::WorkspaceMode,
};

#[test]
fn rebase_requires_the_revision_shown_during_review() {
    for mode in [WorkspaceMode::Standalone, WorkspaceMode::Managed] {
        for document in [Document::Settings, Document::AgentRules] {
            let core = ready(mode);
            edit(&core, document, r#"{"local":true}"#);
            let mut load = request(send(&core, Event::Refresh), document);
            let _effects = resolve_load(&core, &mut load, Some(record(4, r#"{"reviewed":true}"#)));
            let reviewed = editor(&core, document).current.expect("reviewed record");
            let click = Event::Rebase {
                document,
                reviewed_revision: Some(reviewed.revision),
            };
            let mut load = request(send(&core, Event::Refresh), document);
            let _effects = resolve_load(&core, &mut load, Some(record(5, r#"{"unseen":true}"#)));
            let before = editor(&core, document);
            assert!(
                requests(send(&core, click)).is_empty(),
                "review emits no write"
            );
            assert_eq!(
                editor(&core, document),
                before,
                "stale review changes neither draft nor base"
            );
            assert_eq!(
                core.view()
                    .configuration
                    .action_error
                    .expect("stale review error")
                    .kind,
                ConfigurationErrorKind::Conflict,
                "user must review the newly arrived revision"
            );
            assert!(
                !before.actions.contains(&Action::Save),
                "unreviewed conflict blocks saving"
            );
            let _effects = send(
                &core,
                Event::Rebase {
                    document,
                    reviewed_revision: Some(5),
                },
            );
            assert!(
                core.view().configuration.action_error.is_none(),
                "accepted review clears error"
            );
            assert_eq!(
                editor(&core, document).draft.json,
                r#"{"local":true}"#,
                "review keeps local work"
            );
            let mut write = save(&core, document, "reviewed");
            assert!(
                matches!(&write.operation.action, ConfigurationAction::Save(save) if save.expected_revision == Some(5)),
                "write is conditional on the exact reviewed revision"
            );
            let _effects = commit(&core, &mut write, 6);
            assert!(
                !editor(&core, document).dirty,
                "reviewed write commits normally"
            );
        }
    }
}

#[test]
fn review_of_absence_cannot_accept_a_newly_created_document() {
    let core = Core::new();
    for mut load in requests(send(&core, Event::Connect(context(WorkspaceMode::Managed)))) {
        let _effects = resolve_load(&core, &mut load, None);
    }
    edit(&core, Document::Settings, r#"{"local":true}"#);
    let mut load = request(send(&core, Event::Refresh), Document::Settings);
    let _effects = resolve_load(&core, &mut load, Some(record(1, r#"{"created":true}"#)));
    let _effects = send(
        &core,
        Event::Rebase {
            document: Document::Settings,
            reviewed_revision: None,
        },
    );
    let view = editor(&core, Document::Settings);
    assert!(
        view.base_revision.is_none() && view.conflict,
        "reviewed absence keeps create precondition"
    );
    assert!(
        !view.actions.contains(&Action::Save),
        "new record requires explicit review"
    );
}

#[test]
fn continuous_refreshes_allow_saving_and_late_reads_cannot_undo_the_commit() {
    for mode in [WorkspaceMode::Standalone, WorkspaceMode::Managed] {
        for document in [Document::Settings, Document::AgentRules] {
            let core = ready(mode);
            edit(&core, document, r#"{"submitted":true}"#);
            let mut loads = requests(send(&core, Event::Refresh));
            let index = loads
                .iter()
                .position(|load| load.operation.document == document)
                .expect("chosen editor");
            let mut load = loads.remove(index);
            let mut other = loads.pop().expect("independent read");
            for _ in 0..4 {
                assert!(
                    requests(send(&core, Event::Refresh)).is_empty(),
                    "notifications coalesce"
                );
                load = request(
                    resolve_load(&core, &mut load, Some(record(3, "{}"))),
                    document,
                );
                let view = editor(&core, document);
                assert_eq!(
                    view.load,
                    LoadState::Refreshing,
                    "next read is a background refresh"
                );
                assert!(
                    view.actions.contains(&Action::Save),
                    "continuous activity cannot starve saving"
                );
            }
            let mut write = save(&core, document, "busy-workspace");
            let _effects = resolve_load(&core, &mut other, Some(record(4, r#"{"other":true}"#)));
            assert_eq!(
                editor(&core, other.operation.document).base_revision,
                Some(4),
                "other editor read remains owned"
            );
            edit(&core, document, r#"{"newer_draft":true}"#);
            let mut after_save = request(commit(&core, &mut write, 4), document);
            let before = editor(&core, document);
            assert!(
                resolve_load(&core, &mut load, Some(record(3, "{}"))).is_empty(),
                "pre-save read is retired"
            );
            assert_eq!(
                editor(&core, document),
                before,
                "late reply cannot regress state or steal the new read"
            );
            let _effects = resolve_load(
                &core,
                &mut after_save,
                Some(record(5, r#"{"remote":true}"#)),
            );
            let view = editor(&core, document);
            assert_eq!(
                view.save,
                SaveState::Saved(4),
                "provider commit remains visible"
            );
            assert_eq!(
                view.base_revision,
                Some(4),
                "new edits are based on the acknowledged save"
            );
            assert_eq!(
                view.draft.json, r#"{"newer_draft":true}"#,
                "refresh preserves edits during save"
            );
            assert!(view.conflict, "fresh read detects a later external update");
        }
    }
}

#[test]
fn failed_refresh_and_reconnect_reads_block_saving_until_recovered() {
    for mode in [WorkspaceMode::Standalone, WorkspaceMode::Managed] {
        let core = ready(mode);
        edit(&core, Document::Settings, r#"{"local":true}"#);
        let mut load = request(send(&core, Event::Refresh), Document::Settings);
        let _effects = core
            .resolve(
                &mut load,
                Err(ConfigurationError {
                    kind: ConfigurationErrorKind::Unavailable,
                    message: "refresh failed".into(),
                }),
            )
            .expect("read failure");
        assert!(
            !editor(&core, Document::Settings)
                .actions
                .contains(&Action::Save),
            "failed read disables saving"
        );
        let mut recovery = request(send(&core, Event::Refresh), Document::Settings);
        let view = editor(&core, Document::Settings);
        assert_eq!(
            view.load,
            LoadState::Loading,
            "failed read requires recovery"
        );
        assert!(
            !view.actions.contains(&Action::Save),
            "old snapshot cannot bypass recovery"
        );
        let _effects = resolve_load(&core, &mut recovery, Some(record(3, "{}")));
        let mut background = request(send(&core, Event::Refresh), Document::Settings);
        assert!(
            editor(&core, Document::Settings)
                .actions
                .contains(&Action::Save),
            "successful recovery allows background saving"
        );
        let _effects = send(&core, Event::Suspend);
        let mut recovery = request(send(&core, Event::Reconnect), Document::Settings);
        assert!(
            resolve_load(&core, &mut background, Some(record(3, "{}"))).is_empty(),
            "reconnect retires old read"
        );
        let view = editor(&core, Document::Settings);
        assert_eq!(
            view.load,
            LoadState::Loading,
            "reconnect requires an authorized read"
        );
        assert!(
            !view.actions.contains(&Action::Save),
            "old background read cannot complete recovery"
        );
        let _effects = resolve_load(&core, &mut recovery, Some(record(3, "{}")));
        assert!(
            editor(&core, Document::Settings)
                .actions
                .contains(&Action::Save),
            "fresh reconnect read restores saving"
        );
    }
}
