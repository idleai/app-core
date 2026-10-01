use super::{context, ready, reference, request, send, snapshot};
use crate::{
    Core, Effect,
    module::EffectError,
    projections::{
        Event, FreshnessStatus, ProjectionFilter, ProjectionKind, ProjectionLoadState,
        ProjectionSelection,
    },
};

#[test]
fn all_five_views_preserve_supplied_fields_and_filter_without_reclassifying() {
    let core = ready();
    let view = core.view().projections;
    assert_eq!(
        view.load,
        ProjectionLoadState::Ready,
        "validated input is ready"
    );
    for list in [
        &view.activity,
        &view.tasks,
        &view.errors,
        &view.triage,
        &view.need_input,
    ] {
        assert_eq!(list.total, Some(1), "provider total survives");
        assert_eq!(
            list.rows.first().expect("row").sources,
            vec![reference()],
            "full addresses survive"
        );
        assert_eq!(
            list.freshness.generated_at_ms,
            Some(1000),
            "generation time survives"
        );
        assert_eq!(
            list.freshness.checkpoint.as_deref(),
            Some("opaque/checkpoint"),
            "checkpoint remains opaque"
        );
    }
    let selected = ProjectionSelection {
        kind: ProjectionKind::Task,
        key: "stable-row".into(),
    };
    let _effects = send(&core, Event::Select(Some(selected.clone())));
    let _effects = send(
        &core,
        Event::SetFilter {
            kind: ProjectionKind::Task,
            filter: ProjectionFilter {
                text: "missing".into(),
                ..ProjectionFilter::default()
            },
        },
    );
    let view = core.view().projections;
    assert_eq!(view.tasks.visible_count, 0, "filter applies locally");
    assert_eq!(view.tasks.loaded_count, 1, "loaded count is independent");
    assert_eq!(
        view.tasks.total,
        Some(1),
        "filter cannot rewrite scope totals"
    );
    assert_eq!(
        view.selected,
        Some(selected),
        "hidden selection remains stable"
    );
    assert_eq!(
        view.errors.visible_count, 1,
        "other destinations remain independent"
    );
    let _effects = send(
        &core,
        Event::SetFilter {
            kind: ProjectionKind::Task,
            filter: ProjectionFilter {
                text: "🌍".into(),
                status: Some("provider/active".into()),
                labels: vec!["provider/label".into()],
            },
        },
    );
    assert_eq!(
        core.view().projections.tasks.visible_count,
        1,
        "literal title/summary, status and labels are conjunctive"
    );
}

#[test]
fn refresh_replaces_rows_and_preserves_selection_only_while_key_exists() {
    let core = ready();
    let selection = ProjectionSelection {
        kind: ProjectionKind::Task,
        key: "stable-row".into(),
    };
    let _effects = send(&core, Event::Select(Some(selection.clone())));
    let mut first = request(send(&core, Event::Refresh));
    assert_eq!(
        core.view().projections.tasks.freshness.status,
        FreshnessStatus::Stale,
        "pending read marks old result stale"
    );
    let mut second = request(send(&core, Event::Refresh));
    let mut changed = snapshot();
    for input in &mut changed.inputs {
        input
            .rows
            .first_mut()
            .expect("row")
            .sources
            .first_mut()
            .expect("reference")
            .observation = Some("01".repeat(32));
    }
    let _effects = core
        .resolve(&mut second, Ok(changed.clone()))
        .expect("new request");
    let _effects = core
        .resolve(&mut first, Ok(snapshot()))
        .expect("retired request");
    let view = core.view().projections;
    assert_eq!(
        view.selected,
        Some(selection),
        "logical row survives physical update"
    );
    assert_eq!(
        view.tasks
            .rows
            .first()
            .expect("row")
            .sources
            .first()
            .expect("source")
            .observation
            .as_deref(),
        Some("01".repeat(32).as_str()),
        "lower IDs can replace earlier observations"
    );
    assert!(
        !view.needs_refresh,
        "old reply cannot invalidate the new result"
    );
    let mut third = request(send(&core, Event::Refresh));
    for input in &mut changed.inputs {
        input.rows.clear();
        input.total = Some(0);
    }
    let _effects = core.resolve(&mut third, Ok(changed)).expect("retraction");
    assert!(
        core.view().projections.selected.is_none(),
        "retracted selection is removed"
    );
    assert!(
        core.view().projections.tasks.rows.is_empty(),
        "snapshot replaces instead of appending"
    );
}

#[test]
fn invalid_snapshot_is_atomic_and_failed_refresh_retains_stale_rows() {
    let core = ready();
    let mut bad = snapshot();
    bad.inputs
        .last_mut()
        .expect("last input")
        .rows
        .first_mut()
        .expect("row")
        .sources
        .first_mut()
        .expect("reference")
        .item = Some("short".into());
    let mut load = request(send(&core, Event::Refresh));
    let _effects = core
        .resolve(&mut load, Ok(bad))
        .expect("invalid input returned");
    let view = core.view().projections;
    assert!(
        matches!(view.load, ProjectionLoadState::Failed(_)),
        "entire invalid snapshot is rejected"
    );
    assert_eq!(
        view.need_input.rows.first().expect("retained row").sources,
        vec![reference()],
        "no invalid last input is admitted"
    );
    assert_eq!(
        view.tasks.freshness.status,
        FreshnessStatus::Stale,
        "old data is explicitly stale"
    );
    let mut retry = request(send(&core, Event::Refresh));
    let _effects = core
        .resolve(
            &mut retry,
            Err(EffectError {
                message: "Provider unavailable".into(),
            }),
        )
        .expect("host error");
    let mut retry = request(send(&core, Event::Refresh));
    let _effects = core.resolve(&mut retry, Ok(snapshot())).expect("recovery");
    assert_eq!(
        core.view().projections.load,
        ProjectionLoadState::Ready,
        "refresh recovers after failure"
    );
}

#[test]
fn context_retirement_and_suspend_reject_old_continuations() {
    let core = Core::new();
    let mut old = request(send(&core, Event::Connect(context())));
    let mut different = context();
    different.provider = "managed".into();
    different.contributor = "bob".into();
    let mut other = request(send(&core, Event::Connect(different)));
    let mut current = request(send(&core, Event::Connect(context())));
    let _effects = core
        .resolve(&mut old, Ok(snapshot()))
        .expect("old lifetime");
    let _effects = core
        .resolve(&mut other, Ok(snapshot()))
        .expect("old audience");
    assert_eq!(
        core.view().projections.load,
        ProjectionLoadState::Loading,
        "retired results cannot complete current read"
    );
    let _effects = send(&core, Event::Suspend);
    let _effects = core
        .resolve(&mut current, Ok(snapshot()))
        .expect("suspended read");
    assert_eq!(
        core.view().projections.load,
        ProjectionLoadState::Suspended,
        "late results cannot resume transport"
    );
    assert!(
        send(&core, Event::Refresh).is_empty(),
        "no reads during suspension"
    );
    let mut reconnect = request(send(&core, Event::Reconnect));
    let _effects = core
        .resolve(&mut reconnect, Ok(snapshot()))
        .expect("replacement");
    assert_eq!(
        core.view().projections.load,
        ProjectionLoadState::Ready,
        "reconnect reads a fresh snapshot"
    );
}

#[test]
fn invalid_scope_limits_and_selections_do_not_leak_or_request_work() {
    let core = ready();
    let mut load = request(send(&core, Event::Refresh));
    let mut foreign = snapshot();
    foreign.workspace_id = "foreign".into();
    let _effects = core
        .resolve(&mut load, Ok(foreign))
        .expect("mismatched scope");
    assert!(
        matches!(core.view().projections.load, ProjectionLoadState::Failed(_)),
        "wrong workspace rejected even on same chain"
    );
    for limit in [0, 1001] {
        assert!(
            send(&core, Event::SetLimit(limit))
                .iter()
                .all(|effect| matches!(effect, Effect::Render(_))),
            "invalid bounds cannot emit a read"
        );
    }
    let _effects = send(
        &core,
        Event::Select(Some(ProjectionSelection {
            kind: ProjectionKind::Task,
            key: "absent".into(),
        })),
    );
    assert!(
        core.view().projections.action_error.is_some(),
        "invalid selection is explicit"
    );
    let _effects = send(&core, Event::Disconnect);
    assert!(
        core.view().projections.context.is_none(),
        "disconnect clears scope"
    );
    assert!(
        core.view().projections.tasks.rows.is_empty(),
        "disconnect clears rows"
    );
}
