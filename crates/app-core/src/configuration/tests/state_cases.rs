use super::{
    commit, context, edit, ready, record, request, requests, resolve_load, save, send, snapshot,
};
use crate::{
    Core,
    configuration::{
        ConfigurationDocument as Document, ConfigurationEditorAction as Action, ConfigurationError,
        ConfigurationErrorKind, ConfigurationLoadState, ConfigurationResult,
        ConfigurationSaveState, Event,
    },
    workspace::WorkspaceMode,
};

#[test]
fn both_providers_load_edit_and_save_independent_documents() {
    for mode in [WorkspaceMode::Standalone, WorkspaceMode::Managed] {
        let core = ready(mode);
        assert_eq!(
            core.view().configuration.settings.load,
            ConfigurationLoadState::Ready,
            "settings loaded"
        );
        assert_eq!(
            core.view().configuration.agent_rules.load,
            ConfigurationLoadState::Ready,
            "rules loaded"
        );
        edit(
            &core,
            Document::Settings,
            r#"{"name":"Workspace 🌍","future":{"kept":true}}"#,
        );
        edit(
            &core,
            Document::AgentRules,
            r#"{"rules":[{"action":"shell","decision":"ask"}]}"#,
        );
        let mut settings = save(&core, Document::Settings, "settings");
        let mut rules = save(&core, Document::AgentRules, "rules");
        assert_eq!(
            settings.operation.context.mode, mode,
            "selected provider route survives dispatch"
        );
        let view = core.view().configuration;
        assert_eq!(
            view.settings.save,
            ConfigurationSaveState::Saving,
            "save feedback starts before commit"
        );
        assert_eq!(
            view.settings.current,
            Some(record(3, "{}")),
            "dispatch never manufactures a saved version"
        );
        assert!(
            view.settings.pending.is_some() && view.agent_rules.pending.is_some(),
            "both saves are independently pending"
        );
        edit(
            &core,
            Document::Settings,
            r#"{"name":"newer draft","future":{"kept":true}}"#,
        );
        let _effects = commit(&core, &mut settings, 10);
        let _effects = commit(&core, &mut rules, 4);
        let view = core.view().configuration;
        assert_eq!(
            view.settings.save,
            ConfigurationSaveState::Saved(10),
            "authority assigns revision, including jumps"
        );
        assert_eq!(
            view.settings.base_revision,
            Some(10),
            "new edits are based on the acknowledged save"
        );
        assert!(
            view.settings.dirty && view.settings.actions.contains(&Action::Save),
            "edits during save survive and remain savable"
        );
        assert!(
            view.settings.draft.json.contains("newer draft"),
            "late acknowledgement cannot erase newer edits"
        );
        assert!(
            !view.agent_rules.dirty && view.agent_rules.pending.is_none(),
            "other editor becomes clean after commit"
        );
    }
}

#[test]
fn invalid_json_is_editable_but_cannot_be_saved_and_discard_restores_current() {
    let core = ready(WorkspaceMode::Standalone);
    for json in ["{", "[]", "null", "1"] {
        edit(&core, Document::Settings, json);
        let view = core.view().configuration.settings;
        assert_eq!(view.draft.json, json, "intermediate text is preserved");
        assert!(
            view.validation_error.is_some(),
            "form receives syntax feedback"
        );
        assert!(
            !view.actions.contains(&Action::Save),
            "invalid document cannot save"
        );
        assert!(
            requests(send(
                &core,
                Event::Save {
                    document: Document::Settings,
                    request: super::identity("invalid")
                }
            ))
            .is_empty(),
            "invalid intent emits no write"
        );
    }
    let _effects = send(&core, Event::Discard(Document::Settings));
    assert!(
        !core.view().configuration.settings.dirty,
        "discard returns to the saved value"
    );
    assert!(
        core.view()
            .configuration
            .settings
            .validation_error
            .is_none(),
        "discard clears syntax feedback"
    );
}

#[test]
fn load_failure_can_retry_and_absence_uses_a_create_precondition() {
    let core = Core::new();
    let mut loads = requests(send(&core, Event::Connect(context(WorkspaceMode::Managed))));
    let mut load = loads.pop().expect("initial load");
    let document = load.operation.document;
    let _effects = core
        .resolve(
            &mut load,
            Err(ConfigurationError {
                kind: ConfigurationErrorKind::Unavailable,
                message: "offline".into(),
            }),
        )
        .expect("failure");
    let mut load = request(send(&core, Event::Refresh), document);
    let _effects = resolve_load(&core, &mut load, None);
    edit(&core, document, r#"{"new":true}"#);
    let mut write = save(&core, document, "create");
    assert!(
        matches!(&write.operation.action, crate::configuration::ConfigurationAction::Save(save) if save.expected_revision.is_none()),
        "missing documents use create-if-absent"
    );
    let _effects = commit(&core, &mut write, 1);
    assert_eq!(
        core.view().configuration.agent_rules.save,
        ConfigurationSaveState::Saved(1),
        "absent document saved with provider revision"
    );
}

#[test]
fn conflict_preserves_draft_until_explicit_rebase_or_discard() {
    let core = ready(WorkspaceMode::Managed);
    edit(&core, Document::Settings, r#"{"local":true}"#);
    let mut load = request(send(&core, Event::Refresh), Document::Settings);
    let _effects = resolve_load(&core, &mut load, Some(record(4, r#"{"remote":true}"#)));
    let view = core.view().configuration.settings;
    assert!(
        view.conflict && view.dirty,
        "incoming change does not replace local work"
    );
    assert_eq!(
        view.base_revision,
        Some(3),
        "draft retains its original precondition"
    );
    assert!(
        !view.actions.contains(&Action::Save),
        "conflict cannot silently overwrite"
    );
    let _effects = send(&core, Event::Rebase(Document::Settings));
    let view = core.view().configuration.settings;
    assert_eq!(
        view.base_revision,
        Some(4),
        "explicit review adopts current revision"
    );
    assert_eq!(
        view.draft.json, r#"{"local":true}"#,
        "review keeps the draft"
    );
    let mut write = save(&core, Document::Settings, "reviewed");
    let rejected = ConfigurationResult::Rejected {
        request: super::identity("reviewed"),
        error: ConfigurationError {
            kind: ConfigurationErrorKind::Conflict,
            message: "revision changed again".into(),
        },
    };
    let mut reload = request(
        core.resolve(&mut write, Ok(rejected))
            .expect("provider rejection"),
        Document::Settings,
    );
    assert!(
        matches!(
            core.view().configuration.settings.save,
            ConfigurationSaveState::Failed(_)
        ),
        "save error remains visible during reload"
    );
    let _effects = resolve_load(&core, &mut reload, Some(record(5, r#"{"remote":2}"#)));
    let _effects = send(&core, Event::Discard(Document::Settings));
    assert_eq!(
        core.view().configuration.settings.draft.json,
        r#"{"remote":2}"#,
        "discard chooses latest saved value"
    );
    assert!(
        !core.view().configuration.settings.dirty,
        "discard resolves the conflict"
    );
}

#[test]
fn unknown_versions_and_revoked_editing_are_read_only_without_losing_drafts() {
    let core = ready(WorkspaceMode::Managed);
    edit(&core, Document::Settings, r#"{"local":true}"#);
    let mut load = request(send(&core, Event::Refresh), Document::Settings);
    let mut response = snapshot(&load.operation, Some(record(4, "future encoding")));
    response
        .record
        .as_mut()
        .expect("record")
        .value
        .schema_version = 2;
    let _effects = core
        .resolve(&mut load, Ok(ConfigurationResult::Loaded(response)))
        .expect("future version");
    let view = core.view().configuration.settings;
    assert_eq!(
        view.current.expect("future record").value.json,
        "future encoding",
        "unknown format retained verbatim"
    );
    assert!(
        !view.actions.contains(&Action::Save) && !view.actions.contains(&Action::Edit),
        "unknown format cannot be overwritten by this client"
    );
    assert_eq!(
        view.draft.json, r#"{"local":true}"#,
        "format upgrade preserves unsaved text"
    );
    let _effects = send(&core, Event::Discard(Document::Settings));
    assert_eq!(
        core.view()
            .configuration
            .settings
            .validation_error
            .expect("unsupported")
            .kind,
        ConfigurationErrorKind::Unsupported,
        "format feedback is explicit"
    );

    let core = ready(WorkspaceMode::Standalone);
    edit(&core, Document::AgentRules, r#"{"rules":[]}"#);
    let mut load = request(send(&core, Event::Refresh), Document::AgentRules);
    let mut response = snapshot(&load.operation, Some(record(3, "{}")));
    response.can_edit = false;
    let _effects = core
        .resolve(&mut load, Ok(ConfigurationResult::Loaded(response)))
        .expect("capability revocation");
    assert!(
        core.view().configuration.agent_rules.dirty,
        "revocation preserves local work"
    );
    assert!(
        !core
            .view()
            .configuration
            .agent_rules
            .actions
            .contains(&Action::Save),
        "provider decides editing capability"
    );
}
