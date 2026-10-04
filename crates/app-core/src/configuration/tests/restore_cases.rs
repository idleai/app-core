use super::{context, edit, editor, ready, record, requests, save, send};
use crate::{
    Core,
    configuration::{
        ConfigurationDocument, ConfigurationEditorAction, ConfigurationSaveState, Event,
    },
    workspace::WorkspaceMode,
};

#[test]
fn restored_drafts_preserve_the_original_base_and_detect_newer_saved_values() {
    let source = ready(WorkspaceMode::Standalone);
    edit(
        &source,
        ConfigurationDocument::Settings,
        "{\"unknown\":true}",
    );
    let draft = source
        .view()
        .configuration
        .drafts
        .into_iter()
        .next()
        .expect("dirty draft");
    let reopened = Core::new();
    for mut load in requests(send(
        &reopened,
        Event::Connect(context(WorkspaceMode::Standalone)),
    )) {
        let _effects =
            super::resolve_load(&reopened, &mut load, Some(record(4, "{\"changed\":true}")));
    }
    let _effects = send(&reopened, Event::Restore(draft.clone()));
    let restored = editor(&reopened, ConfigurationDocument::Settings);
    assert!(
        restored.dirty && restored.conflict,
        "newer saved content requires conflict review"
    );
    assert_eq!(
        restored.base_revision,
        Some(3),
        "the original base survives restart"
    );
    assert_eq!(
        restored.draft, draft.value,
        "the complete draft survives restart"
    );
    assert!(
        !restored.actions.contains(&ConfigurationEditorAction::Save),
        "a conflict cannot silently overwrite current data"
    );
    assert_eq!(
        reopened.view().configuration.drafts,
        vec![draft],
        "persistence remains stable after restoration"
    );
}

#[test]
fn restored_uncertain_save_reuses_the_original_payload_and_deadline() {
    let source = ready(WorkspaceMode::Standalone);
    edit(&source, ConfigurationDocument::AgentRules, "{\"rule\":1}");
    let original = save(&source, ConfigurationDocument::AgentRules, "original").operation;
    edit(&source, ConfigurationDocument::AgentRules, "{\"rule\":2}");
    let draft = source
        .view()
        .configuration
        .drafts
        .into_iter()
        .next()
        .expect("pending draft");
    let reopened = ready(WorkspaceMode::Standalone);
    assert!(
        requests(send(&reopened, Event::Restore(draft))).is_empty(),
        "restoration does not execute a write"
    );
    assert!(
        matches!(
            editor(&reopened, ConfigurationDocument::AgentRules).save,
            ConfigurationSaveState::Uncertain(_)
        ),
        "a lost acknowledgement remains uncertain"
    );
    let retry = requests(send(
        &reopened,
        Event::RetrySave(ConfigurationDocument::AgentRules),
    ))
    .pop()
    .expect("original retry");
    assert_eq!(
        retry.operation, original,
        "retry keeps its original identity, deadline, revision and value"
    );
    assert_eq!(
        editor(&reopened, ConfigurationDocument::AgentRules)
            .draft
            .json,
        "{\"rule\":2}",
        "edits made during saving survive recovery"
    );
}

#[test]
fn restore_cannot_replace_new_edits_or_cross_a_context_boundary() {
    let source = ready(WorkspaceMode::Standalone);
    edit(&source, ConfigurationDocument::Settings, "{broken");
    let mut draft = source
        .view()
        .configuration
        .drafts
        .into_iter()
        .next()
        .expect("invalid intermediate draft");
    let reopened = ready(WorkspaceMode::Standalone);
    draft.context.contributor_id = "someone-else".into();
    let _effects = send(&reopened, Event::Restore(draft.clone()));
    assert!(
        !editor(&reopened, ConfigurationDocument::Settings).dirty,
        "another contributor's draft is rejected"
    );
    draft.context = context(WorkspaceMode::Standalone);
    edit(&reopened, ConfigurationDocument::Settings, "{\"new\":true}");
    let _effects = send(&reopened, Event::Restore(draft));
    assert_eq!(
        editor(&reopened, ConfigurationDocument::Settings)
            .draft
            .json,
        "{\"new\":true}",
        "new edits take precedence over a late restore"
    );
}

#[test]
fn delayed_restore_recovers_the_original_save_without_replacing_new_edits() {
    for (document, newer) in [
        (ConfigurationDocument::Settings, r#"{"new":true}"#),
        (ConfigurationDocument::AgentRules, "{unfinished"),
    ] {
        let source = ready(WorkspaceMode::Standalone);
        edit(&source, document, r#"{"original":true}"#);
        let original = save(&source, document, "lost-reply").operation;
        let retained = source
            .view()
            .configuration
            .drafts
            .into_iter()
            .next()
            .expect("pending draft");
        let reopened = Core::new();
        for mut load in requests(send(
            &reopened,
            Event::Connect(context(WorkspaceMode::Standalone)),
        )) {
            let _effects = super::resolve_load(
                &reopened,
                &mut load,
                Some(record(4, r#"{"original":true}"#)),
            );
        }
        edit(&reopened, document, newer);
        let _effects = send(&reopened, Event::Restore(retained.clone()));
        let actual = editor(&reopened, document);
        assert_eq!(
            actual.draft.json, newer,
            "preserve edits made during recovery"
        );
        assert_eq!(
            actual.pending, retained.pending,
            "preserve the exact unresolved save"
        );
        assert!(
            actual
                .actions
                .contains(&ConfigurationEditorAction::RetrySave)
                && !actual.actions.contains(&ConfigurationEditorAction::Save),
            "only the original request can be recovered"
        );
        let persisted = reopened
            .view()
            .configuration
            .drafts
            .into_iter()
            .next()
            .expect("recoverable draft");
        let restarted = ready(WorkspaceMode::Standalone);
        let _effects = send(&restarted, Event::Restore(persisted));
        for core in [&reopened, &restarted] {
            let mut retry = requests(send(core, Event::RetrySave(document)))
                .pop()
                .expect("original retry");
            assert_eq!(
                retry.operation, original,
                "identity, deadline, base and payload survive"
            );
            let _effects = super::commit(core, &mut retry, 4);
            let actual = editor(core, document);
            assert_eq!(
                actual.draft.json, newer,
                "recovery preserves newer draft text"
            );
            assert!(
                actual.pending.is_none(),
                "confirmed outcome clears uncertainty"
            );
        }
    }
}
