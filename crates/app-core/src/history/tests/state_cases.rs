use super::{first, id, message, observation, page, send};
use crate::{
    Core,
    history::{
        BlockState, Event, Filter, HistoryPage, QueryAction, QueryResult, RequestState,
        SearchMatch, SearchPage, Selected,
    },
    module::EffectError,
};

#[test]
fn logical_identity_disclosure_and_replay_survive_pages_and_refresh() {
    let core = Core::new();
    let mut initial = first(send(&core, Event::Connect("chain-a".into())));
    // Hash order is opposite the content dependency: the append arrives first.
    let append = message(2, Some(9), b" world");
    page(
        &core,
        &mut initial,
        std::slice::from_ref(&append),
        Some(id(2)),
    );
    let item = core
        .view()
        .history
        .items
        .into_iter()
        .next()
        .expect("logical row");
    assert_eq!(item.key, id(200).to_string(), "key is the logical item");
    assert!(
        matches!(
            item.blocks.first().map(|block| &block.state),
            Some(BlockState::Content {
                complete: false,
                ..
            })
        ),
        "missing prefix remains partial"
    );

    let mut more = first(send(&core, Event::LoadMore));
    assert!(
        send(&core, Event::LoadMore).is_empty(),
        "duplicate load is suppressed"
    );
    let start = message(9, None, b"hello");
    page(&core, &mut more, std::slice::from_ref(&start), None);
    let item = core
        .view()
        .history
        .items
        .into_iter()
        .next()
        .expect("same logical row");
    assert_eq!(item.observations.len(), 2, "observations remain distinct");
    assert!(
        matches!(item.blocks.first().map(|block| &block.state), Some(BlockState::Content { bytes, complete: true, finished: false, head }) if bytes == b"hello world" && *head == id(2).to_string()),
        "core StreamState uses predecessors rather than largest hash"
    );

    let mut item_query = first(send(&core, Event::ToggleDisclosure(id(200).to_string())));
    page(&core, &mut item_query, &[append, start], None);
    assert!(
        send(
            &core,
            Event::Select(Selected {
                item: Some(id(200).to_string()),
                observation: None
            })
        )
        .is_empty(),
        "cached item does not reload"
    );
    let refresh = send(&core, Event::Refresh);
    assert_eq!(refresh.len(), 2, "fresh history and selected item scans");
    assert_eq!(
        core.view().history.selected.item,
        Some(id(200).to_string()),
        "selection survives cache invalidation"
    );
    assert_eq!(
        core.view().history.expanded,
        vec![id(200).to_string()],
        "disclosure uses stable keys"
    );
}

#[test]
fn cancelled_search_and_old_chain_results_cannot_mutate_current_context() {
    let core = Core::new();
    let mut old = first(send(&core, Event::Connect("a".into())));
    let mut old_search = first(send(&core, Event::Search("old".into())));
    let mut new_search = first(send(&core, Event::Search("new".into())));
    let _renders = core
        .resolve(
            &mut old_search,
            Ok(QueryResult::Search(SearchPage {
                matches: vec![SearchMatch {
                    observation: observation(&message(1, None, b"old")),
                    fields: Vec::new(),
                }],
                scanned: 1,
                ..SearchPage::default()
            })),
        )
        .expect("old search response");
    assert!(
        core.view().history.search.matches.is_empty(),
        "superseded search is discarded"
    );
    let _other = send(&core, Event::Connect("b".into()));
    let _again = send(&core, Event::Connect("a".into()));
    page(&core, &mut old, &[message(1, None, b"old context")], None);
    let _renders = core
        .resolve(
            &mut new_search,
            Err(EffectError {
                message: "late error".into(),
            }),
        )
        .expect("old context error");
    assert!(
        core.view().history.items.is_empty(),
        "A -> B -> A does not reuse request identity"
    );
    assert_eq!(
        core.view().history.search.text,
        "",
        "other context cannot restore a search"
    );
}

#[test]
fn paging_keeps_empty_filtered_continuations_and_retry_preserves_cache() {
    let core = Core::new();
    let _initial = send(&core, Event::Connect("a".into()));
    let mut filtered = first(send(
        &core,
        Event::SetFilter(Filter {
            author: Some(id(203).to_string()),
            ..Filter::default()
        }),
    ));
    let _renders = core
        .resolve(
            &mut filtered,
            Ok(QueryResult::History(HistoryPage {
                observations: Vec::new(),
                scanned: 100,
                next_after: Some(id(100).to_string()),
            })),
        )
        .expect("empty filtered page");
    assert!(
        !core.view().history.paging.exhausted,
        "zero rows do not imply exhaustion"
    );
    let mut next = first(send(&core, Event::LoadMore));
    assert!(
        matches!(&next.operation.action, QueryAction::History { page, .. } if page.after == Some(id(100).to_string())),
        "continue after the inspected candidate"
    );
    page(
        &core,
        &mut next,
        &[message(101, None, b"known")],
        Some(id(101)),
    );
    let mut failing = first(send(&core, Event::LoadMore));
    let _renders = core
        .resolve(
            &mut failing,
            Err(EffectError {
                message: "disconnected".into(),
            }),
        )
        .expect("failure");
    assert_eq!(
        core.view().history.items.len(),
        1,
        "page failure retains cached rows"
    );
    assert!(
        matches!(core.view().history.paging.state, RequestState::Failed(_)),
        "failure is visible"
    );
    let retry = first(send(&core, Event::LoadMore));
    assert_eq!(
        retry.operation, failing.operation,
        "retry does not skip the failed page"
    );
}

#[test]
fn wrong_response_and_nonadvancing_cursor_fail_without_inserting_rows() {
    let core = Core::new();
    let mut initial = first(send(&core, Event::Connect("a".into())));
    let _renders = core
        .resolve(&mut initial, Ok(QueryResult::Search(SearchPage::default())))
        .expect("wrong response");
    assert!(
        matches!(core.view().history.paging.state, RequestState::Failed(_)),
        "mismatched union variant fails the request"
    );
    let mut retry = first(send(&core, Event::LoadMore));
    page(&core, &mut retry, &[message(1, None, b"ok")], Some(id(1)));
    let mut next = first(send(&core, Event::LoadMore));
    page(&core, &mut next, &[message(2, None, b"bad")], Some(id(1)));
    assert_eq!(
        core.view()
            .history
            .items
            .first()
            .expect("cached item")
            .observations
            .len(),
        1,
        "invalid page is rejected atomically"
    );
    assert!(
        matches!(core.view().history.paging.state, RequestState::Failed(_)),
        "bad cursor is visible and retryable"
    );
}

#[test]
fn search_navigation_retains_full_identity_and_wraps_without_row_coordinates() {
    let core = Core::new();
    let _initial = send(&core, Event::Connect("a".into()));
    let mut query = first(send(&core, Event::Search("hello".into())));
    let _renders = core
        .resolve(
            &mut query,
            Ok(QueryResult::Search(SearchPage {
                matches: vec![SearchMatch {
                    observation: observation(&message(1, None, b"hello")),
                    fields: Vec::new(),
                }],
                scanned: 1,
                ..SearchPage::default()
            })),
        )
        .expect("search");
    let requests = send(&core, Event::NavigateMatch(-1));
    assert_eq!(
        requests.len(),
        2,
        "navigation requests item context, stored records and field content"
    );
    let view = core.view().history;
    assert_eq!(view.search.cursor, Some(0), "single match wraps");
    assert_eq!(
        view.selected.item,
        Some(id(200).to_string()),
        "logical selection"
    );
    assert_eq!(
        view.selected.observation,
        Some(id(1).to_string()),
        "observation drill-down stays distinct"
    );
    assert!(
        send(&core, Event::NavigateMatch(1)).is_empty(),
        "pending results are reused"
    );
}

#[test]
fn every_causal_parent_and_logical_cause_is_retained() {
    let core = Core::new();
    let mut initial = first(send(&core, Event::Connect("a".into())));
    let op = message(1, None, b"three parents");
    let mut activity = editchain_core::activity::Operation::view(&op).expect("activity");
    activity.parents = vec![id(4), id(3), id(2)];
    activity.causes = vec![editchain_core::activity::ItemId(id(205))];
    page(
        &core,
        &mut initial,
        &[activity.into_op().expect("valid envelope")],
        None,
    );
    let row = core
        .view()
        .history
        .items
        .into_iter()
        .next()
        .expect("item")
        .observations
        .into_iter()
        .next()
        .expect("observation");
    assert_eq!(
        row.parents,
        vec![id(4).to_string(), id(3).to_string(), id(2).to_string()],
        "no two-parent truncation"
    );
    assert_eq!(
        row.causes,
        vec![id(205).to_string()],
        "logical causes remain distinct"
    );
    assert_ne!(
        row.author, row.recorder,
        "author and recorder are not conflated"
    );
}

#[test]
fn tool_attempts_and_channels_replay_as_distinct_blocks() {
    use editchain_core::{
        Payload,
        activity::{
            ContentUpdate, ItemId, Kind, Operation, OutputChannel, Stage, Tool, UpdateMode,
        },
    };
    let core = Core::new();
    let mut initial = first(send(&core, Event::Connect("a".into())));
    let operations = [
        (1, 210, 220, OutputChannel::Stdout, b"stdout".as_slice()),
        (2, 210, 221, OutputChannel::Stderr, b"stderr".as_slice()),
        (3, 211, 220, OutputChannel::Stdout, b"retry".as_slice()),
    ]
    .into_iter()
    .map(|(number, attempt, block, channel, bytes)| {
        Operation::new(
            id(number),
            ItemId(id(200)),
            ItemId(id(201)),
            Kind::Tool(Tool {
                native_call: Payload::Inline(b"provider-call".to_vec()),
                name: Payload::Inline(b"terminal".to_vec()),
                stage: Stage::Snapshot,
                attempt: ItemId(id(attempt)),
                parent_call: None,
                arguments: Payload::Empty,
                channel,
                output: Some(ContentUpdate {
                    block: ItemId(id(block)),
                    position: None,
                    mode: UpdateMode::Replace,
                    previous: None,
                    media_type: Payload::Empty,
                    content: Payload::Inline(bytes.to_vec()),
                }),
                terminal: None,
                outcome: None,
            }),
        )
        .into_op()
        .expect("valid tool observation")
    })
    .collect::<Vec<_>>();
    page(&core, &mut initial, &operations, None);
    let item = core
        .view()
        .history
        .items
        .into_iter()
        .next()
        .expect("call item");
    assert_eq!(
        item.blocks.len(),
        3,
        "attempt and block identities remain distinct"
    );
    assert!(
        item.blocks
            .iter()
            .any(|block| block.attempt == Some(id(210).to_string())
                && block.channel.as_deref() == Some("Stderr")
                && matches!(&block.state, BlockState::Content { bytes, .. } if bytes == b"stderr")),
        "stderr is not merged into stdout or another attempt"
    );
    assert_eq!(
        item.observations
            .first()
            .expect("tool")
            .preview
            .as_ref()
            .expect("preview")
            .text,
        "terminal",
        "tool label does not become the native call ID"
    );
}

#[test]
fn cross_item_predecessors_and_divergent_streams_are_not_complete_content() {
    use editchain_core::activity::{ItemId, Operation};
    let core = Core::new();
    let mut initial = first(send(&core, Event::Connect("a".into())));
    let mut foreign = Operation::view(&message(1, None, b"foreign")).expect("base");
    foreign.item = ItemId(id(212));
    page(
        &core,
        &mut initial,
        &[
            foreign.into_op().expect("foreign item"),
            message(2, Some(1), b"suffix"),
        ],
        None,
    );
    let selected = core
        .view()
        .history
        .items
        .into_iter()
        .find(|item| item.key == id(200).to_string())
        .expect("target");
    assert!(
        matches!(
            selected.blocks.first().map(|block| &block.state),
            Some(BlockState::Conflicted(_))
        ),
        "global replay detects a predecessor from another logical item"
    );

    let mut fresh = first(send(&core, Event::Refresh));
    page(
        &core,
        &mut fresh,
        &[
            message(1, None, b"base"),
            message(2, Some(1), b"left"),
            message(3, Some(1), b"right"),
        ],
        None,
    );
    let item = core
        .view()
        .history
        .items
        .into_iter()
        .next()
        .expect("branched item");
    assert!(
        matches!(
            item.blocks.first().map(|block| &block.state),
            Some(BlockState::Conflicted(_))
        ),
        "unjoined branches cannot be settled by hash order"
    );
}

#[test]
fn bounded_cache_pins_selection_and_disclosure_and_allows_evicted_items_to_reload() {
    use editchain_core::{
        OpId,
        activity::{ItemId, Operation},
    };
    fn numbered(number: u32) -> editchain_core::Op {
        let mut activity = Operation::view(&message(1, None, b"cached")).expect("activity");
        activity.id = OpId::from_display_str(&format!("{number:064x}")).expect("full ID");
        activity.item = ItemId(activity.id);
        activity.into_op().expect("valid item")
    }
    let core = Core::new();
    let operations: Vec<_> = (1..=2100).map(numbered).collect();
    let selected_id = numbered(1).id.to_string();
    let evicted_id = numbered(2).id.to_string();
    let expanded_id = numbered(3).id.to_string();
    let mut initial = first(send(&core, Event::Connect("a".into())));
    let first_page = operations.get(..100).expect("first page");
    page(
        &core,
        &mut initial,
        first_page,
        first_page.last().map(|op| op.id),
    );
    let mut item = first(send(
        &core,
        Event::Select(Selected {
            item: Some(selected_id.clone()),
            observation: None,
        }),
    ));
    page(&core, &mut item, &[numbered(1)], None);
    let mut item = first(send(&core, Event::LoadItem(evicted_id.clone())));
    page(&core, &mut item, &[numbered(2)], None);
    let mut expanded = first(send(&core, Event::ToggleDisclosure(expanded_id.clone())));
    page(&core, &mut expanded, &[numbered(3)], None);
    for chunk in operations.get(100..).expect("remaining pages").chunks(100) {
        let mut request = first(send(&core, Event::LoadMore));
        page(
            &core,
            &mut request,
            chunk,
            chunk
                .last()
                .filter(|op| op.id != numbered(2100).id)
                .map(|op| op.id),
        );
    }
    let view = core.view().history;
    assert_eq!(
        view.cache.observations, 2000,
        "unselected observations are bounded"
    );
    assert_eq!(view.cache.evicted, 100, "eviction is explicit for clients");
    assert_eq!(
        view.selected_item.expect("pinned selection").key,
        selected_id,
        "selected item retained"
    );
    assert!(
        view.items
            .iter()
            .any(|item| item.key == expanded_id && item.expanded),
        "disclosed item retained"
    );
    let reload = first(send(
        &core,
        Event::Select(Selected {
            item: Some(evicted_id.clone()),
            observation: None,
        }),
    ));
    assert!(
        matches!(reload.operation.action, QueryAction::Item { item, page } if item == evicted_id && page.after.is_none()),
        "eviction invalidates earlier complete-item paging state"
    );
}
