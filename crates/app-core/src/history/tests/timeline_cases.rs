//! Timeline state checks are independent of native routing and viewport pixels.

use crux_core::Request;
use idle_history::timeline::{
    Action, Address, Cursor, Geometry, Group, Match, Matches, Position, Response, Row, Source,
    Target, VERSION, Window,
};

use super::{first, requests, send};
use crate::{
    Core,
    history::{
        Event, OpenTarget, Query, QueryAction, QueryResult, RecordRef, RequestState,
        timeline::{Event as TimelineEvent, Surface},
    },
    module::EffectError,
};

fn emit(core: &Core, event: TimelineEvent) -> Vec<Request<Query>> {
    send(core, Event::Timeline(event))
}

fn row(value: u64) -> Row {
    let address = Address {
        source: Source::Current,
        record: RecordRef {
            operation: format!("{value:064x}"),
            hash: format!("{:064x}", value.saturating_add(1)),
        },
    };
    Row {
        occurrence: format!("current:{}", address.record.operation),
        item: address.record.operation.clone(),
        records: vec![address.clone()],
        address: Target::Record(address),
        kind: idle_history::query::ActivityKind::Message,
        title: "Activity".into(),
        preview: format!("Work {value}"),
        author: "Local author".into(),
        session: "Task".into(),
        tags: Vec::new(),
        timestamp: Some(value),
        open: OpenTarget::OperationJson,
        unavailable: None,
        group: None,
        relationships: Vec::new(),
        graph: Geometry::default(),
    }
}

fn cursor(offset: u64) -> Cursor {
    Cursor {
        revision: "r1".into(),
        view: "v1".into(),
        offset,
    }
}

fn window(offset: u64, count: u64) -> Window {
    Window {
        version: VERSION,
        revision: "r1".into(),
        rows: (offset..offset.saturating_add(count)).map(row).collect(),
        newer: (offset > 0).then(|| cursor(offset.saturating_sub(200))),
        older: Some(cursor(offset.saturating_add(count))),
        offset,
        activities: 10_000,
        visible_rows: 10_000,
        max_lane: 100,
        gaps: Vec::new(),
        rebuilding: None,
    }
}

fn reply(core: &Core, request: &mut Request<Query>, response: Response) -> Vec<Request<Query>> {
    requests(
        core.resolve(request, Ok(QueryResult::Timeline(Box::new(response))))
            .expect("timeline result"),
    )
}

fn setup() -> (Core, Request<Query>) {
    let core = Core::new();
    let _legacy = send(&core, Event::Connect("a".into()));
    let request = first(emit(&core, TimelineEvent::Load(Surface::Editor)));
    (core, request)
}

#[test]
fn subscription_binding_refreshes_mounted_surfaces_and_retires_their_earlier_reads() {
    use crate::subscriptions::{Context, Event as SubscriptionEvent, SubscriptionResult};

    let (core, mut earlier_editor) = setup();
    let mut earlier_mini = first(emit(&core, TimelineEvent::Load(Surface::Mini)));
    let mut join = core
        .process_event(crate::Event::Subscriptions(SubscriptionEvent::Connect(
            Context {
                provider: "provider".into(),
                workspace: "workspace".into(),
                contributor: "person".into(),
                chain: "a".into(),
            },
        )))
        .into_iter()
        .find_map(|effect| {
            if let crate::Effect::Subscription(request) = effect {
                Some(*request)
            } else {
                None
            }
        })
        .expect("subscription join");
    for earlier in [&mut earlier_editor, &mut earlier_mini] {
        assert!(reply(&core, earlier, Response::Window(window(400, 20))).is_empty());
    }
    let before = core.view().history.timeline;
    assert!(before.editor.window.is_none() && before.mini.window.is_none());
    let refreshed = requests(
        core.resolve(
            &mut join,
            Ok(SubscriptionResult::Joined {
                connection: "connected".into(),
            }),
        )
        .expect("joined subscription"),
    );
    let mut limits = Vec::new();
    for mut request in refreshed {
        if let QueryAction::Timeline(query) = &request.operation.action
            && let Action::Window { limit, .. } = query.action
        {
            limits.push(limit);
            assert!(
                reply(
                    &core,
                    &mut request,
                    Response::Window(window(0, u64::from(limit)))
                )
                .is_empty()
            );
        }
    }
    limits.sort_unstable();
    assert_eq!(
        limits,
        vec![40, 200],
        "both mounted views reload after joining"
    );
    assert_eq!(
        core.view()
            .history
            .timeline
            .mini
            .window
            .expect("refreshed mini")
            .rows
            .len(),
        40
    );
}

#[test]
fn mini_handoff_seeks_the_exact_record_and_keeps_viewports_independent() {
    let (core, mut editor) = setup();
    assert!(reply(&core, &mut editor, Response::Window(window(400, 200))).is_empty());
    let mut mini = first(emit(&core, TimelineEvent::Load(Surface::Mini)));
    assert!(
        matches!(&mini.operation.action, QueryAction::Timeline(request) if matches!(request.action, Action::Window { limit: 40, .. }))
    );
    let mut recent = window(0, 40);
    let record = recent.rows.get_mut(3).expect("mini row");
    let mut address = record.address.record().expect("record fixture").clone();
    address.source = Source::Retained;
    record.address = Target::Record(address);
    let exact = recent.rows.get(3).expect("mini row").clone();
    assert!(reply(&core, &mut mini, Response::Window(recent)).is_empty());
    assert!(
        emit(
            &core,
            TimelineEvent::Select {
                surface: Surface::Mini,
                occurrence: exact.occurrence.clone(),
                open: false
            }
        )
        .is_empty()
    );
    let selected = core
        .view()
        .history
        .timeline
        .selected
        .expect("exact mini selection");
    assert_eq!(selected.address, exact.address);
    let mut seek = first(emit(
        &core,
        TimelineEvent::Reveal {
            surface: Surface::Editor,
            selection: selected,
        },
    ));
    assert!(
        matches!(&seek.operation.action, QueryAction::Timeline(request) if matches!(&request.action, Action::Window { position: Position::Seek(id), .. } if *id == exact.occurrence))
    );
    assert_eq!(
        core.view()
            .history
            .timeline
            .mini
            .window
            .expect("mini window")
            .rows
            .len(),
        40
    );
    let mut target = window(0, 20);
    target.rows.get_mut(3).expect("selected row").address = exact.address.clone();
    assert!(reply(&core, &mut seek, Response::Window(target)).is_empty());
    assert_eq!(
        core.view()
            .history
            .timeline
            .selected
            .expect("editor selection")
            .address,
        exact.address
    );
    assert_eq!(
        core.view().history.timeline.editor.focus,
        Some(exact.occurrence)
    );
    assert_eq!(
        core.view().history.timeline.open,
        RequestState::Idle,
        "revealing an entry does not open its native document"
    );
}

#[test]
fn native_activation_is_exact_and_superseded_results_cannot_replace_selection() {
    let (core, mut initial) = setup();
    let mut rows = window(0, 3);
    if let Some(row) = rows.rows.get_mut(1) {
        let mut address = row.address.record().expect("record fixture").clone();
        address.source = Source::Retained;
        row.address = Target::Record(address);
        row.open = OpenTarget::Diff;
    }
    assert!(reply(&core, &mut initial, Response::Window(rows)).is_empty());
    let mut first_open = first(emit(
        &core,
        TimelineEvent::Select {
            surface: Surface::Editor,
            occurrence: row(0).occurrence,
            open: true,
        },
    ));
    let mut latest_open = first(emit(
        &core,
        TimelineEvent::Select {
            surface: Surface::Editor,
            occurrence: row(1).occurrence.clone(),
            open: true,
        },
    ));
    assert!(
        matches!(&latest_open.operation.action, QueryAction::OpenAt { address, target: OpenTarget::Diff } if address.record().is_some_and(|address| address.source == Source::Retained && Some(&address.record) == row(1).address.record().map(|address| &address.record)))
    );
    let _renders = core
        .resolve(&mut latest_open, Ok(QueryResult::Opened))
        .expect("latest open");
    let _renders = core
        .resolve(
            &mut first_open,
            Err(EffectError {
                message: "superseded failure".into(),
            }),
        )
        .expect("old result");
    let state = core.view().history.timeline;
    assert_eq!(
        state.selected.expect("selection").occurrence,
        row(1).occurrence
    );
    assert_eq!(state.open, RequestState::Ready);
    assert!(
        emit(
            &core,
            TimelineEvent::Move {
                surface: Surface::Editor,
                delta: 1
            }
        )
        .is_empty(),
        "arrows select without activating"
    );
    assert_eq!(
        core.view()
            .history
            .timeline
            .selected
            .expect("moved selection")
            .occurrence,
        row(2).occurrence
    );
}

#[test]
fn a_git_selection_keeps_its_repository_and_full_commit_without_a_record_address() {
    let (core, mut initial) = setup();
    let address = Target::Commit {
        repository: "123".into(),
        oid: "a".repeat(40),
    };
    let mut rows = window(0, 1);
    let commit = rows.rows.first_mut().expect("commit row");
    commit.address = address.clone();
    commit.records.clear();
    commit.kind = crate::history::ActivityKind::Commit;
    commit.open = OpenTarget::Record;
    let occurrence = commit.occurrence.clone();
    assert!(reply(&core, &mut initial, Response::Window(rows)).is_empty());
    let request = first(emit(
        &core,
        TimelineEvent::Select {
            surface: Surface::Editor,
            occurrence,
            open: true,
        },
    ));
    assert_eq!(
        request.operation.action,
        QueryAction::OpenAt {
            address: address.clone(),
            target: OpenTarget::Record
        }
    );
    assert_eq!(
        core.view()
            .history
            .timeline
            .selected
            .expect("selected commit")
            .address,
        address
    );
}

fn grouped(state: (bool, bool)) -> Window {
    let (live, expanded) = state;
    let mut value = window(0, 3);
    for (index, row) in value.rows.iter_mut().enumerate() {
        row.group = Some(Group {
            id: "task-a".into(),
            count: 3,
            summary: String::new(),
            live,
            expanded,
            header: index == 0,
            entry: "entry".into(),
            exit: "exit".into(),
        });
    }
    value
}

#[test]
fn completion_preserves_a_visible_group_and_find_temporarily_opens_a_manual_fold() {
    let (core, mut initial) = setup();
    assert!(reply(&core, &mut initial, Response::Window(grouped((true, true)))).is_empty());
    let _requests = emit(
        &core,
        TimelineEvent::Visible {
            surface: Surface::Editor,
            anchor: Some(row(1).occurrence),
            groups: vec!["task-a".into()],
            at_newest: false,
        },
    );
    let mut refresh = first(emit(&core, TimelineEvent::Refresh(Surface::Editor)));
    let mut protected = first(reply(
        &core,
        &mut refresh,
        Response::Window(grouped((false, false))),
    ));
    assert!(
        matches!(&protected.operation.action, QueryAction::Timeline(request) if matches!(&request.action, Action::Window { view, .. } if view.disclosures.iter().any(|choice| choice.group == "task-a" && choice.expanded)))
    );
    assert!(
        reply(
            &core,
            &mut protected,
            Response::Window(grouped((false, true)))
        )
        .is_empty()
    );
    let mut toggle = first(emit(
        &core,
        TimelineEvent::Toggle {
            surface: Surface::Editor,
            group: "task-a".into(),
        },
    ));
    assert!(
        reply(
            &core,
            &mut toggle,
            Response::Window(grouped((false, false)))
        )
        .is_empty()
    );
    let mut find = first(emit(
        &core,
        TimelineEvent::Find {
            surface: Surface::Editor,
            text: "Work".into(),
        },
    ));
    let mut seek = first(reply(
        &core,
        &mut find,
        Response::Found(Matches {
            revision: "r1".into(),
            matches: vec![Match {
                occurrence: row(1).occurrence,
                address: row(1).address,
                group: Some("task-a".into()),
                preview: "Work".into(),
            }],
            total: 1,
            next: None,
            unavailable: 0,
        }),
    ));
    assert!(
        matches!(&seek.operation.action, QueryAction::Timeline(request) if matches!(&request.action, Action::Window { position: Position::Seek(_), view, .. } if view.disclosures.iter().any(|choice| choice.group == "task-a" && choice.expanded))),
        "Find must override the manual fold temporarily"
    );
    assert!(reply(&core, &mut seek, Response::Window(grouped((false, true)))).is_empty());
    let restored = first(emit(
        &core,
        TimelineEvent::Find {
            surface: Surface::Editor,
            text: String::new(),
        },
    ));
    assert!(
        matches!(&restored.operation.action, QueryAction::Timeline(request) if matches!(&request.action, Action::Window { view, .. } if view.disclosures.iter().any(|choice| choice.group == "task-a" && !choice.expanded))),
        "closing Find restores the manual choice"
    );
}

#[test]
fn paging_is_bounded_and_keyboard_navigation_crosses_windows_without_activation() {
    let (core, mut initial) = setup();
    assert!(reply(&core, &mut initial, Response::Window(window(0, 200))).is_empty());
    let _selection = emit(
        &core,
        TimelineEvent::Select {
            surface: Surface::Editor,
            occurrence: row(199).occurrence,
            open: false,
        },
    );
    let mut next = first(emit(
        &core,
        TimelineEvent::Move {
            surface: Surface::Editor,
            delta: 1,
        },
    ));
    assert!(reply(&core, &mut next, Response::Window(window(200, 200))).is_empty());
    assert_eq!(
        core.view()
            .history
            .timeline
            .selected
            .expect("next row")
            .occurrence,
        row(200).occurrence
    );
    for offset in (400u64..=2_000).step_by(200) {
        let mut next = first(emit(
            &core,
            TimelineEvent::Page {
                surface: Surface::Editor,
                newer: false,
            },
        ));
        assert!(reply(&core, &mut next, Response::Window(window(offset, 200))).is_empty());
    }
    let before = core.view().history.timeline.editor;
    assert!(before.window.as_ref().expect("window").rows.len() <= 1_000);
    assert!(
        before.cached_bytes
            <= u64::try_from(idle_history::timeline::MAX_CACHED_BYTES / 2).expect("limit")
    );
    let mut failed = first(emit(
        &core,
        TimelineEvent::Page {
            surface: Surface::Editor,
            newer: false,
        },
    ));
    let _renders = core
        .resolve(
            &mut failed,
            Err(EffectError {
                message: "Temporary disconnect".into(),
            }),
        )
        .expect("failed page");
    let after = core.view().history.timeline.editor;
    assert_eq!(
        before.window, after.window,
        "a failed page preserves readable rows and cursors"
    );
    assert!(matches!(after.state, RequestState::Failed(_)));
}

#[test]
fn switching_workspaces_and_out_of_order_windows_cannot_restore_old_rows() {
    let (core, mut initial) = setup();
    let mut seek = first(emit(
        &core,
        TimelineEvent::Seek {
            surface: Surface::Editor,
            occurrence: row(500).occurrence,
        },
    ));
    assert!(reply(&core, &mut initial, Response::Window(window(0, 200))).is_empty());
    assert!(
        core.view().history.timeline.editor.window.is_none(),
        "superseded latest response is ignored"
    );
    let _connection = send(&core, Event::Connect("b".into()));
    let _connection = send(&core, Event::Connect("a".into()));
    assert!(reply(&core, &mut seek, Response::Window(window(400, 200))).is_empty());
    assert!(
        core.view().history.timeline.editor.window.is_none(),
        "A to B to A retires pending request identities"
    );
}
