use super::{
    commit, context, edit, editor, identity, ready, record, request, requests, resolve_load, save,
    send, snapshot,
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
fn accepted_retry_clears_action_errors_and_retires_background_reads() {
    for mode in [WorkspaceMode::Standalone, WorkspaceMode::Managed] {
        for document in [Document::Settings, Document::AgentRules] {
            let core = ready(mode);
            edit(&core, document, r#"{"saved":true}"#);
            let mut original = save(&core, document, "lost-reply");
            let _effects = core
                .resolve(
                    &mut original,
                    Err(ConfigurationError {
                        kind: ConfigurationErrorKind::Unavailable,
                        message: "reply lost after send".into(),
                    }),
                )
                .expect("unknown outcome");
            let mut background = request(send(&core, Event::Refresh), document);
            let _effects = send(&core, Event::Refresh);
            background = request(
                resolve_load(&core, &mut background, Some(record(3, "{}"))),
                document,
            );
            assert_eq!(
                editor(&core, document).load,
                ConfigurationLoadState::Refreshing,
                "retry is offered during continuous activity"
            );
            assert!(
                editor(&core, document).actions.contains(&Action::RetrySave),
                "background refresh cannot starve recovery"
            );
            let _effects = send(&core, Event::Discard(document));
            assert!(
                core.view().configuration.action_error.is_some(),
                "unresolved save cannot be discarded"
            );
            let mut retry = request(send(&core, Event::RetrySave(document)), document);
            assert_eq!(
                retry.operation, original.operation,
                "recovery uses the unchanged original request"
            );
            assert!(
                core.view().configuration.action_error.is_none(),
                "accepted retry clears obsolete action feedback"
            );
            let before = editor(&core, document);
            let effects = core
                .resolve(
                    &mut background,
                    Err(ConfigurationError {
                        kind: ConfigurationErrorKind::Forbidden,
                        message: "old read refused".into(),
                    }),
                )
                .expect("retired read failure");
            assert!(
                effects.is_empty(),
                "retired failure cannot change current state"
            );
            assert_eq!(
                editor(&core, document),
                before,
                "retired error cannot interrupt the retry"
            );
            let mut fresh = request(commit(&core, &mut retry, 4), document);
            assert_eq!(
                editor(&core, document).save,
                ConfigurationSaveState::Saved(4),
                "retry confirms committed revision"
            );
            assert!(
                core.view().configuration.action_error.is_none(),
                "successful retry has no stale action error"
            );
            let _effects = resolve_load(&core, &mut fresh, Some(record(4, r#"{"saved":true}"#)));
            assert!(
                !editor(&core, document).dirty,
                "fresh read confirms the committed value"
            );
        }
    }
}

#[test]
fn interrupted_save_retains_identity_payload_and_new_edits_across_reconnect() {
    let core = ready(WorkspaceMode::Standalone);
    edit(&core, Document::Settings, r#"{"saved":true}"#);
    let mut original = save(&core, Document::Settings, "interrupted");
    let operation = original.operation.clone();
    edit(&core, Document::Settings, r#"{"newer":true}"#);
    let _effects = send(&core, Event::Suspend);
    assert!(
        matches!(
            core.view().configuration.settings.save,
            ConfigurationSaveState::Uncertain(_)
        ),
        "connection loss does not claim failure or success"
    );
    assert!(
        requests(send(&core, Event::RetrySave(Document::Settings))).is_empty(),
        "retry waits for a fresh authorized read"
    );
    let _effects = commit(&core, &mut original, 4);
    assert_eq!(
        core.view().configuration.settings.current,
        Some(record(3, "{}")),
        "retired continuation cannot affect the suspended editor"
    );
    let mut load = request(send(&core, Event::Reconnect), Document::Settings);
    let _effects = resolve_load(&core, &mut load, Some(record(5, r#"{"third_party":true}"#)));
    let view = core.view().configuration.settings;
    assert!(
        view.actions.contains(&Action::RetrySave) && !view.actions.contains(&Action::Save),
        "only original save can be recovered"
    );
    let mut retry = request(
        send(&core, Event::RetrySave(Document::Settings)),
        Document::Settings,
    );
    assert_eq!(
        retry.operation, operation,
        "retry retains payload, deadline and revision even after remote changes"
    );
    let _effects = commit(&core, &mut retry, 4);
    let view = core.view().configuration.settings;
    assert_eq!(
        view.save,
        ConfigurationSaveState::Saved(4),
        "original retained result resolves uncertainty"
    );
    assert_eq!(
        view.current,
        Some(record(5, r#"{"third_party":true}"#)),
        "older retained save cannot roll back newer discovery"
    );
    assert_eq!(
        view.draft.json, r#"{"newer":true}"#,
        "edits made during the first attempt survive retry"
    );
    assert!(
        view.conflict,
        "remaining edits require review against the newer revision"
    );
}

#[test]
fn transport_failure_never_discards_pending_save_or_substitutes_a_new_request() {
    let core = ready(WorkspaceMode::Managed);
    edit(&core, Document::AgentRules, r#"{"rules":[]}"#);
    let mut original = save(&core, Document::AgentRules, "unknown");
    let _effects = core
        .resolve(
            &mut original,
            Err(ConfigurationError {
                kind: ConfigurationErrorKind::Unavailable,
                message: "connection closed after send".into(),
            }),
        )
        .expect("uncertain response");
    assert!(
        core.view().configuration.agent_rules.pending.is_some(),
        "transport failure may follow a durable commit"
    );
    assert!(
        requests(send(
            &core,
            Event::Save {
                document: Document::AgentRules,
                request: identity("replacement")
            }
        ))
        .is_empty(),
        "a second logical mutation cannot replace an unknown first one"
    );
    let _effects = send(&core, Event::Discard(Document::AgentRules));
    assert!(
        core.view().configuration.agent_rules.pending.is_some(),
        "discard cannot hide unresolved work"
    );
    let mut retry = request(
        send(&core, Event::RetrySave(Document::AgentRules)),
        Document::AgentRules,
    );
    assert_eq!(
        retry.operation, original.operation,
        "manual retry resends unchanged request"
    );
    let rejected = ConfigurationResult::Rejected {
        request: identity("unknown"),
        error: ConfigurationError {
            kind: ConfigurationErrorKind::InvalidInput,
            message: "Rule rejected by provider".into(),
        },
    };
    let _effects = core
        .resolve(&mut retry, Ok(rejected))
        .expect("retained refusal");
    let view = core.view().configuration.agent_rules;
    assert!(
        view.pending.is_none() && view.dirty,
        "definite rejection clears pending save but keeps edits"
    );
    assert!(
        matches!(view.save, ConfigurationSaveState::Failed(_)),
        "provider refusal remains visible"
    );
}

#[test]
fn refreshes_coalesce_and_wait_for_in_flight_saves() {
    let core = ready(WorkspaceMode::Standalone);
    edit(&core, Document::Settings, r#"{"changed":true}"#);
    let mut write = save(&core, Document::Settings, "saving");
    for _ in 0..3 {
        assert!(
            requests(send(&core, Event::Refresh))
                .iter()
                .all(|request| request.operation.document != Document::Settings),
            "settings read waits until save result"
        );
    }
    let mut load = request(commit(&core, &mut write, 4), Document::Settings);
    for _ in 0..3 {
        let _effects = send(&core, Event::Refresh);
    }
    let mut followup = request(
        resolve_load(&core, &mut load, Some(record(4, r#"{"changed":true}"#))),
        Document::Settings,
    );
    assert!(
        requests(resolve_load(
            &core,
            &mut followup,
            Some(record(5, r#"{"fresh":true}"#))
        ))
        .is_empty(),
        "a burst requires just one followup read"
    );
    assert_eq!(
        core.view().configuration.settings.draft.json,
        r#"{"fresh":true}"#,
        "clean editor follows external updates"
    );
}

#[test]
fn context_switches_retire_even_same_context_late_results() {
    let core = Core::new();
    let active = context(WorkspaceMode::Standalone);
    let mut old = request(
        send(&core, Event::Connect(active.clone())),
        Document::Settings,
    );
    let _effects = send(&core, Event::Disconnect);
    let mut current = request(send(&core, Event::Connect(active)), Document::Settings);
    let _effects = resolve_load(&core, &mut old, Some(record(99, r#"{"old":true}"#)));
    assert_eq!(
        core.view().configuration.settings.load,
        ConfigurationLoadState::Loading,
        "old lifetime cannot steal new load"
    );
    let _effects = resolve_load(&core, &mut current, Some(record(3, "{}")));
    assert_eq!(
        core.view().configuration.settings.current,
        Some(record(3, "{}")),
        "new lifetime owns the value"
    );
}

#[test]
fn malformed_scoped_and_regressing_reads_leave_previous_draft_intact() {
    for case in 0..5 {
        let core = ready(WorkspaceMode::Managed);
        edit(&core, Document::Settings, r#"{"draft":true}"#);
        let mut load = request(send(&core, Event::Refresh), Document::Settings);
        let mut response = snapshot(&load.operation, Some(record(4, "{}")));
        match case {
            0 => response.context.contributor_id = "other".into(),
            1 => response.document = Document::AgentRules,
            2 => response.record = Some(record(2, "{}")),
            3 => response.record = Some(record(3, r#"{"changed_same_revision":true}"#)),
            _ => response.record = Some(record(4, "[]")),
        }
        let _effects = core
            .resolve(&mut load, Ok(ConfigurationResult::Loaded(response)))
            .expect("invalid response");
        let view = core.view().configuration.settings;
        assert!(
            matches!(view.load, ConfigurationLoadState::Failed(_)),
            "bad result produces read error"
        );
        assert_eq!(
            view.current,
            Some(record(3, "{}")),
            "last confirmed record preserved"
        );
        assert_eq!(
            view.draft.json, r#"{"draft":true}"#,
            "draft is not replaced by bad data"
        );
    }
}

#[test]
fn malformed_save_acknowledgements_remain_uncertain_and_keys_cannot_be_reused() {
    let core = ready(WorkspaceMode::Managed);
    edit(&core, Document::Settings, r#"{"saved":true}"#);
    let mut write = save(&core, Document::Settings, "once");
    let result = ConfigurationResult::Saved {
        request: identity("foreign"),
        snapshot: snapshot(&write.operation, Some(record(4, r#"{"saved":true}"#))),
    };
    let _effects = core
        .resolve(&mut write, Ok(result))
        .expect("mismatched request");
    assert!(
        matches!(
            core.view().configuration.settings.save,
            ConfigurationSaveState::Uncertain(_)
        ),
        "wrong retry identity cannot establish success"
    );
    let mut retry = request(
        send(&core, Event::RetrySave(Document::Settings)),
        Document::Settings,
    );
    let result = ConfigurationResult::Saved {
        request: identity("once"),
        snapshot: snapshot(&retry.operation, Some(record(4, "{}"))),
    };
    let _effects = core
        .resolve(&mut retry, Ok(result))
        .expect("mismatched payload");
    assert!(
        core.view().configuration.settings.pending.is_some(),
        "mismatched committed value remains uncertain"
    );
    let mut retry = request(
        send(&core, Event::RetrySave(Document::Settings)),
        Document::Settings,
    );
    let _effects = commit(&core, &mut retry, 4);
    edit(&core, Document::AgentRules, r#"{"rules":[]}"#);
    assert!(
        requests(send(
            &core,
            Event::Save {
                document: Document::AgentRules,
                request: identity("once")
            }
        ))
        .is_empty(),
        "mutation identity cannot be reused across document kinds"
    );
}

#[test]
fn retained_save_cannot_restore_a_capability_revoked_after_the_commit() {
    let core = ready(WorkspaceMode::Managed);
    edit(&core, Document::Settings, r#"{"saved":true}"#);
    let _original = save(&core, Document::Settings, "lost-reply");
    let _effects = send(&core, Event::Suspend);
    let mut load = request(send(&core, Event::Reconnect), Document::Settings);
    let mut response = snapshot(&load.operation, Some(record(4, r#"{"saved":true}"#)));
    response.can_edit = false;
    let _effects = core
        .resolve(&mut load, Ok(ConfigurationResult::Loaded(response)))
        .expect("current permissions");
    let mut retry = request(
        send(&core, Event::RetrySave(Document::Settings)),
        Document::Settings,
    );
    let _effects = commit(&core, &mut retry, 4);
    let view = core.view().configuration.settings;
    assert_eq!(
        view.save,
        ConfigurationSaveState::Saved(4),
        "retained outcome resolves the original save"
    );
    assert!(
        !view.actions.contains(&Action::Edit),
        "historical commit capability cannot override fresh discovery"
    );
}
