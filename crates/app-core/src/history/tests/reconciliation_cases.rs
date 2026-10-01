use editchain_core::{
    Op, Payload,
    activity::{ItemId, Kind, Operation},
};
use editchain_engine::Engine;

use super::{
    engine_cases::{operation_details, run},
    first, id, message, send,
};
use crate::{
    Core,
    history::{
        self, BlockState, ContentValue, Event, QueryAction, QueryResult, RecordLookupStatus,
        RequestState, Selected,
    },
};

fn separate_item(number: u8, item: u8, bytes: &[u8]) -> Op {
    let mut activity = Operation::view(&message(number, None, bytes)).expect("message fixture");
    activity.item = ItemId(id(item));
    activity.into_op().expect("separate item")
}

#[test]
fn reconciliation_replaces_every_loaded_page_and_keeps_match_identity() {
    let directory = tempfile::tempdir().expect("chain");
    let engine = Engine::open(directory.path()).expect("engine");
    for number in 30..=150 {
        let _saved = engine
            .append(&message(number, None, b"needle"))
            .expect("record");
    }
    let mut queries = engine.queries().expect("queries");
    let core = Core::new();
    run(&core, &mut queries, Event::Connect("chain".into()));
    run(&core, &mut queries, Event::LoadMore);
    run(&core, &mut queries, Event::Search("needle".into()));
    run(&core, &mut queries, Event::SearchMore);
    run(&core, &mut queries, Event::NavigateMatch(1));
    let selected = core.view().history.selected;
    let _saved = engine
        .append(&message(10, None, b"needle"))
        .expect("lower ID");
    run(&core, &mut queries, Event::Reconnect);
    let view = core.view().history;
    assert_eq!(
        view.paging.scanned, 122,
        "all previously loaded candidate pages are replaced"
    );
    assert_eq!(
        view.search.matches.len(),
        122,
        "all loaded search pages are replaced"
    );
    assert_eq!(
        view.search.cursor,
        Some(1),
        "current match follows its stable record"
    );
    assert_eq!(
        view.selected, selected,
        "selection does not move with ID ordering"
    );
    assert_eq!(
        view.items.len(),
        1,
        "observations group into one visible logical item"
    );
    run(&core, &mut queries, Event::Refresh);
    assert_eq!(core.view().history, view, "multi-page replay is idempotent");
}

#[test]
fn details_requested_during_recovery_are_included_in_the_new_snapshot() {
    let directory = tempfile::tempdir().expect("chain");
    let engine = Engine::open(directory.path()).expect("engine");
    let _saved = engine
        .append(&message(50, None, b"details"))
        .expect("record");
    let mut queries = engine.queries().expect("queries");
    let core = Core::new();
    run(&core, &mut queries, Event::Connect("chain".into()));
    let mut old = first(send(&core, Event::Refresh));
    let old_result = history::engine::execute(&mut queries, "chain", &old.operation);
    let mut current = first(send(
        &core,
        Event::LoadOperationDetails {
            operation: id(50).to_string(),
            refresh: false,
        },
    ));
    assert!(
        matches!(&current.operation.action, QueryAction::Reconcile(request) if request.details.contains(&id(50).to_string())),
        "a detail click during recovery is not lost"
    );
    let _effects = core
        .resolve(&mut old, old_result)
        .expect("retired snapshot");
    let response = history::engine::execute(&mut queries, "chain", &current.operation);
    let _effects = core
        .resolve(&mut current, response)
        .expect("current snapshot");
    assert_eq!(
        operation_details(&core, 50).status,
        RecordLookupStatus::Found,
        "requested details arrive"
    );
}

#[test]
fn search_navigation_during_recovery_supersedes_the_pending_snapshot() {
    let directory = tempfile::tempdir().expect("chain");
    let engine = Engine::open(directory.path()).expect("engine");
    let _saved = engine
        .append(&separate_item(50, 210, b"needle"))
        .expect("record");
    let mut queries = engine.queries().expect("queries");
    let core = Core::new();
    run(&core, &mut queries, Event::Connect("chain".into()));
    run(&core, &mut queries, Event::Search("needle".into()));
    let mut old = first(send(&core, Event::Refresh));
    let old_result = history::engine::execute(&mut queries, "chain", &old.operation);
    let mut current = first(send(&core, Event::NavigateMatch(1)));
    assert!(
        matches!(&current.operation.action, QueryAction::Reconcile(request)
            if request.details.contains(&id(50).to_string())
                && request.items.iter().any(|item| item.item == id(210).to_string())),
        "navigation loads its selected item and details through reconciliation"
    );
    let selected = core.view().history.selected;
    let _effects = core
        .resolve(&mut old, old_result)
        .expect("retired snapshot");
    assert_eq!(
        core.view().history.reconciliation,
        RequestState::Loading,
        "old snapshot cannot finish the current recovery"
    );
    let response = history::engine::execute(&mut queries, "chain", &current.operation);
    let _effects = core
        .resolve(&mut current, response)
        .expect("current snapshot");
    assert_eq!(core.view().history.selected, selected, "selection survives");
    assert_eq!(core.view().history.search.cursor, Some(0), "match survives");
    assert_eq!(
        operation_details(&core, 50).status,
        RecordLookupStatus::Found,
        "navigation's requested details arrive"
    );
}

#[test]
fn replay_reconciles_lower_ids_retractions_and_late_blobs_without_duplicates() {
    let directory = tempfile::tempdir().expect("chain directory");
    let engine = Engine::open(directory.path()).expect("engine");
    let blob_source = tempfile::tempdir().expect("blob source");
    let source = Engine::open(blob_source.path()).expect("source engine");
    let late = source.store_blob(b" late needle").expect("address");
    let _saved = engine
        .append(&message(80, None, b"base needle"))
        .expect("base");
    let mut append = Operation::view(&message(90, Some(80), b"unused")).expect("append");
    if let Kind::Message(message) = &mut append.kind {
        message.blocks.first_mut().expect("block").content = Payload::Blob(late);
    }
    let _saved = engine
        .append(&append.into_op().expect("append operation"))
        .expect("append missing content");
    let _saved = engine
        .append(&separate_item(100, 210, b"will conflict needle"))
        .expect("independent item");
    let mut queries = engine.queries().expect("queries");
    let core = Core::new();
    run(&core, &mut queries, Event::Connect("chain".into()));
    run(
        &core,
        &mut queries,
        Event::ToggleDisclosure(id(200).to_string()),
    );
    run(
        &core,
        &mut queries,
        Event::Select(Selected {
            item: Some(id(200).to_string()),
            observation: Some(id(90).to_string()),
        }),
    );
    run(&core, &mut queries, Event::Search("needle".into()));
    run(&core, &mut queries, Event::NavigateMatch(1));
    let selection = core.view().history.selected;
    assert!(
        operation_details(&core, 90)
            .fields
            .iter()
            .any(|field| field.value == ContentValue::Missing),
        "initial gap"
    );
    let _saved = engine
        .append(&separate_item(10, 211, b"lower needle"))
        .expect("late lower ID");
    let _saved = engine
        .append(&separate_item(100, 210, b"conflicting variant"))
        .expect("quarantine");
    let _saved = engine.store_blob(b" late needle").expect("late blob");

    let before = core.view().history;
    let mut request = first(send(&core, Event::Reconnect));
    assert_eq!(
        core.view().history.items,
        before.items,
        "cached rows remain until replacement is validated"
    );
    let response =
        history::engine::execute(&mut queries, "chain", &request.operation).expect("snapshot");
    let _effects = core.resolve(&mut request, Ok(response)).expect("replace");
    let view = core.view().history;
    assert_eq!(view.selected, selection, "stable selection");
    assert_eq!(view.expanded, before.expanded, "stable disclosure");
    assert_eq!(
        view.items.len(),
        2,
        "new lower item replaces quarantined item"
    );
    assert_eq!(
        view.items.first().expect("lower item").key,
        id(211).to_string(),
        "scan restarts below old page position"
    );
    assert!(
        !view
            .items
            .iter()
            .any(|item| item.key == id(210).to_string()),
        "quarantined canonical row retracted"
    );
    assert_eq!(
        operation_details(&core, 100).status,
        RecordLookupStatus::Conflicted,
        "raw variants remain inspectable"
    );
    assert!(
        operation_details(&core, 90)
            .fields
            .iter()
            .any(|field| field.value == ContentValue::Available(b" late needle".to_vec())),
        "cached details receive late blobs"
    );
    assert_eq!(
        view.search.matches.len(),
        3,
        "lower-ID and late-blob hits arrive; retracted hit disappears"
    );
    assert!(
        view.search.unavailable.is_empty(),
        "old missing-content search results are removed"
    );
    let selected = view.selected_item.as_ref().expect("selected item");
    assert!(
        matches!(selected.blocks.first().map(|block| &block.state), Some(BlockState::Content { bytes, .. }) if bytes == b"base needle late needle"),
        "append replay uses each operation once"
    );

    run(&core, &mut queries, Event::Refresh);
    assert_eq!(
        core.view().history,
        view,
        "repeated reconciliation does not duplicate rows, matches, blocks or details"
    );
}

#[test]
fn a_conflicted_selected_record_keeps_its_anchor_but_retracts_its_content() {
    let directory = tempfile::tempdir().expect("chain");
    let engine = Engine::open(directory.path()).expect("engine");
    let _saved = engine
        .append(&message(50, None, b"original"))
        .expect("message");
    let mut queries = engine.queries().expect("queries");
    let core = Core::new();
    run(&core, &mut queries, Event::Connect("chain".into()));
    run(
        &core,
        &mut queries,
        Event::Select(Selected {
            item: Some(id(200).to_string()),
            observation: Some(id(50).to_string()),
        }),
    );
    let selection = core.view().history.selected;
    let _saved = engine
        .append(&message(50, None, b"variant"))
        .expect("conflict");
    run(&core, &mut queries, Event::Reconnect);
    assert_eq!(
        core.view().history.selected,
        selection,
        "selected record identity persists"
    );
    assert!(
        core.view().history.items.is_empty(),
        "canonical list retracts conflicted row"
    );
    let selected = core.view().history.selected_item.expect("conflict anchor");
    assert!(
        matches!(
            selected.blocks.first().map(|block| &block.state),
            Some(BlockState::Conflicted(_))
        ),
        "old bytes are not displayed as accepted content"
    );
    let before = core.view().history;
    run(&core, &mut queries, Event::Refresh);
    assert_eq!(
        core.view().history,
        before,
        "conflict replay preserves the same anchor"
    );
}

#[test]
fn malformed_replacement_and_old_search_context_cannot_publish_partial_rows() {
    let directory = tempfile::tempdir().expect("chain");
    let engine = Engine::open(directory.path()).expect("engine");
    let _saved = engine
        .append(&message(50, None, b"known"))
        .expect("message");
    let mut queries = engine.queries().expect("queries");
    let core = Core::new();
    run(&core, &mut queries, Event::Connect("chain".into()));
    run(&core, &mut queries, Event::Search("known".into()));
    let before = core.view().history;
    let mut request = first(send(&core, Event::Refresh));
    let response =
        history::engine::execute(&mut queries, "chain", &request.operation).expect("snapshot");
    let mut snapshot = if let QueryResult::Reconciled(snapshot) = response {
        Some(snapshot)
    } else {
        None
    }
    .expect("replacement snapshot");
    snapshot.search.push(history::SearchPage::default());
    let _effects = core
        .resolve(&mut request, Ok(QueryResult::Reconciled(snapshot)))
        .expect("malformed replacement");
    assert_eq!(
        core.view().history.items,
        before.items,
        "history pages do not leak from rejected replacement"
    );
    assert_eq!(
        core.view().history.search,
        before.search,
        "search results stay intact"
    );
    assert!(
        matches!(core.view().history.reconciliation, RequestState::Failed(_)),
        "failure remains retryable"
    );

    let mut old = first(send(&core, Event::Refresh));
    let old_result = history::engine::execute(&mut queries, "chain", &old.operation);
    let mut new = first(send(&core, Event::Search("new".into())));
    assert!(
        matches!(&new.operation.action, QueryAction::Reconcile(request) if request.text == "new"),
        "search change supersedes reconciliation"
    );
    let before = core.view().history;
    let _effects = core.resolve(&mut old, old_result).expect("old snapshot");
    assert_eq!(
        core.view().history,
        before,
        "old search snapshot is discarded"
    );
    let result = history::engine::execute(&mut queries, "chain", &new.operation);
    let _effects = core.resolve(&mut new, result).expect("current snapshot");
    assert!(
        core.view().history.search.matches.is_empty(),
        "current search wins"
    );
}
