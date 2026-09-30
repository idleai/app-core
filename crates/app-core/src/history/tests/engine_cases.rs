use editchain_core::{
    ActorId, Clock, FileEdit, FileOp, FileStage, MessageOp, Op, OpKind, ParentSet, PathId, Payload,
    ScopeRef, Tags,
    activity::{ItemId, Kind, Operation, Original, OriginalRef},
};
use editchain_engine::{Engine, queries::ChainQueries};

use super::{first, id, message, send};
use crate::{
    Core,
    history::{
        self, BlockState, Comparison, Event, Evidence, EvidenceState, EvidenceStatus,
        EvidenceValue, Filter, Query, QueryAction, QueryResult, RequestState, Selected,
    },
};

fn run(core: &Core, queries: &mut ChainQueries, event: Event) {
    for mut request in send(core, event) {
        let result = history::engine::execute(queries, "chain", &request.operation);
        let follow_up = core.resolve(&mut request, result).expect("host result");
        assert!(
            super::requests(follow_up).is_empty(),
            "host completion emits render only"
        );
    }
}

fn evidence(core: &Core, operation: u8) -> Evidence {
    let entry = core
        .view()
        .history
        .evidence
        .into_iter()
        .find(|value| value.operation == id(operation).to_string())
        .expect("cached evidence");
    if let EvidenceState::Ready(value) = entry.state {
        Some(value)
    } else {
        None
    }
    .expect("ready evidence")
}

#[test]
fn native_engine_pages_searches_and_replays_mixed_legacy_and_activity_history() {
    let directory = tempfile::tempdir().expect("temporary chain");
    let engine = Engine::open(directory.path()).expect("open engine");
    let mut writer = engine.writer().expect("writer");
    for number in 1..=101 {
        let text = if number == 101 {
            b"needle".as_slice()
        } else {
            b"plain".as_slice()
        };
        let _admission = writer
            .append(&message(number, None, text))
            .expect("append observation");
    }
    drop(writer);
    let legacy = Op {
        id: id(102),
        source: None,
        actor: ActorId(5),
        clock: Clock::None,
        parents: ParentSet::None,
        scope: ScopeRef::None,
        tags: Tags::NONE,
        kind: OpKind::Message(MessageOp {
            content: Payload::Inline(b"legacy needle".to_vec()),
            content_type: Payload::Empty,
        }),
    };
    let _admission = engine.append(&legacy).expect("append legacy");
    let mut queries = engine.queries().expect("queries");
    let core = Core::new();
    run(&core, &mut queries, Event::Connect("chain".into()));
    assert_eq!(
        core.view().history.paging.scanned,
        100,
        "bounded candidates"
    );
    run(&core, &mut queries, Event::Search("needle".into()));
    assert!(
        core.view().history.search.matches.is_empty(),
        "first search page has no matches"
    );
    assert!(
        !core.view().history.search.paging.exhausted,
        "search continues beyond no-hit page"
    );
    run(&core, &mut queries, Event::SearchMore);
    assert_eq!(
        core.view().history.search.matches.len(),
        2,
        "schema-three and legacy reads both match"
    );
    let legacy_item = Operation::view(&legacy)
        .expect("legacy adapter")
        .item
        .to_string();
    assert!(
        core.view()
            .history
            .search
            .matches
            .iter()
            .any(|value| value.item == legacy_item),
        "shared legacy identity adapter is used"
    );
    run(
        &core,
        &mut queries,
        Event::SetFilter(Filter {
            author: Some(ItemId::legacy("author", 5).to_string()),
            ..Filter::default()
        }),
    );
    assert!(
        core.view().history.items.is_empty(),
        "empty filtered candidate page"
    );
    run(&core, &mut queries, Event::LoadMore);
    assert_eq!(
        core.view().history.items.first().expect("legacy item").key,
        legacy_item,
        "filters do not exclude legacy records via schema-only indexes"
    );
}

#[test]
fn late_blobs_exact_originals_binary_empty_and_missing_evidence_remain_distinct() {
    let directory = tempfile::tempdir().expect("temporary chain");
    let engine = Engine::open(directory.path()).expect("open engine");
    let late_bytes = b"late evidence\0\xff\r\n";
    // Obtain a real content address from a different chain, without storing it here.
    let other = tempfile::tempdir().expect("blob source");
    let source = Engine::open(other.path()).expect("source engine");
    let blob = source.store_blob(late_bytes).expect("content address");
    let mut activity = Operation::view(&message(1, None, b"unused")).expect("message");
    if let Kind::Message(message) = &mut activity.kind {
        message.blocks.first_mut().expect("block").content = Payload::Blob(blob);
    }
    activity.original = Some(OriginalRef {
        operation: id(2),
        converter: "fixture-v1".into(),
    });
    let _admission = engine
        .append(&activity.into_op().expect("valid activity"))
        .expect("append message");
    let original_bytes = b" {\"exact\": true} \r\n\xff";
    let original = Operation::new(
        id(2),
        ItemId(id(210)),
        ItemId(id(201)),
        Kind::Original(Original {
            provider: "fixture".into(),
            format: None,
            native: Vec::new(),
            location: None,
            bytes: Payload::Inline(original_bytes.to_vec()),
            hash: None,
        }),
    )
    .into_op()
    .expect("original");
    let _admission = engine.append(&original).expect("append original");
    let _admission = engine
        .append(&message(3, None, b""))
        .expect("known empty replacement");
    let mut queries = engine.queries().expect("queries");
    let core = Core::new();
    run(&core, &mut queries, Event::Connect("chain".into()));
    run(
        &core,
        &mut queries,
        Event::LoadEvidence {
            operation: id(1).to_string(),
            refresh: false,
        },
    );
    let before = evidence(&core, 1);
    assert!(
        before
            .fields
            .iter()
            .any(|field| field.value == EvidenceValue::Missing),
        "blob gap is explicit"
    );
    run(&core, &mut queries, Event::Search("late".into()));
    assert!(
        core.view().history.search.matches.is_empty(),
        "missing bytes cannot match"
    );
    assert!(
        !core.view().history.search.unavailable.is_empty(),
        "search gaps are retained"
    );
    let _blob = engine.store_blob(late_bytes).expect("late blob arrives");
    run(
        &core,
        &mut queries,
        Event::LoadEvidence {
            operation: id(1).to_string(),
            refresh: true,
        },
    );
    let after = evidence(&core, 1);
    assert_eq!(
        before.observation, after.observation,
        "content arrival does not change identity"
    );
    assert!(
        after
            .fields
            .iter()
            .any(|field| field.value == EvidenceValue::Available(late_bytes.to_vec())),
        "binary content is exact"
    );
    run(
        &core,
        &mut queries,
        Event::LoadEvidence {
            operation: id(2).to_string(),
            refresh: false,
        },
    );
    let raw = evidence(&core, 2);
    assert!(
        raw.fields
            .iter()
            .any(|field| field.value == EvidenceValue::Available(original_bytes.to_vec())),
        "Original bytes preserve whitespace and non-UTF8"
    );
    let retained = queries.record_variants(id(2)).expect("retained bytes");
    assert_eq!(
        raw.records.first().expect("original encoding").bytes,
        retained.first().expect("engine encoding").encoded,
        "raw evidence is stored encoding, not JSON reserialization"
    );
    run(
        &core,
        &mut queries,
        Event::LoadEvidence {
            operation: id(3).to_string(),
            refresh: false,
        },
    );
    assert!(
        evidence(&core, 3)
            .fields
            .iter()
            .any(|field| field.value == EvidenceValue::Available(Vec::new())),
        "empty bytes are available"
    );
    run(
        &core,
        &mut queries,
        Event::LoadEvidence {
            operation: id(99).to_string(),
            refresh: false,
        },
    );
    assert_eq!(
        evidence(&core, 99).status,
        EvidenceStatus::Missing,
        "missing observation differs from empty content"
    );
}

#[test]
fn quarantined_observations_retract_replayed_content_without_losing_selection() {
    let directory = tempfile::tempdir().expect("temporary chain");
    let engine = Engine::open(directory.path()).expect("open engine");
    let _admission = engine
        .append(&message(1, None, b"accepted"))
        .expect("append message");
    let mut queries = engine.queries().expect("queries");
    let core = Core::new();
    run(&core, &mut queries, Event::Connect("chain".into()));
    run(
        &core,
        &mut queries,
        Event::Select(Selected {
            item: Some(id(200).to_string()),
            observation: Some(id(1).to_string()),
        }),
    );
    let _conflict = engine
        .append(&message(1, None, b"conflicting"))
        .expect("retain conflict");
    run(
        &core,
        &mut queries,
        Event::LoadEvidence {
            operation: id(1).to_string(),
            refresh: true,
        },
    );
    let evidence = evidence(&core, 1);
    assert_eq!(
        evidence.status,
        EvidenceStatus::Conflicted,
        "quarantine is explicit"
    );
    assert_eq!(evidence.records.len(), 2, "both exact variants retained");
    assert!(
        evidence.observation.is_none(),
        "no canonical variant invented"
    );
    let selected = core
        .view()
        .history
        .selected_item
        .expect("stable selection anchor");
    assert_eq!(
        selected.key,
        id(200).to_string(),
        "logical selection survives retraction"
    );
    assert!(
        matches!(
            selected.blocks.first().map(|block| &block.state),
            Some(BlockState::Conflicted(_))
        ),
        "old stream bytes are no longer facts"
    );
    assert!(
        selected
            .observations
            .first()
            .expect("evidence anchor")
            .preview
            .is_none(),
        "quarantined preview is retracted"
    );
}

#[test]
fn file_diff_uses_recorded_snapshots_and_native_actions_need_a_host_capability() {
    let directory = tempfile::tempdir().expect("temporary chain");
    let engine = Engine::open(directory.path()).expect("open engine");
    let before = engine.store_blob(b"old\0\xff").expect("before");
    let after = engine.store_blob(b"new\0\xff").expect("after");
    let op = Op {
        id: id(1),
        source: None,
        actor: ActorId(1),
        clock: Clock::None,
        parents: ParentSet::None,
        scope: ScopeRef::None,
        tags: Tags::NONE,
        kind: OpKind::File(FileOp {
            path: PathId(5),
            stage: FileStage::Applied,
            base: Some(before.id),
            after: Some(after.id),
            edit: FileEdit::None,
        }),
    };
    let _admission = engine.append(&op).expect("file revision");
    let mut queries = engine.queries().expect("queries");
    let core = Core::new();
    run(&core, &mut queries, Event::Connect("chain".into()));
    run(
        &core,
        &mut queries,
        Event::LoadEvidence {
            operation: id(1).to_string(),
            refresh: false,
        },
    );
    let evidence = evidence(&core, 1);
    assert!(
        matches!(evidence.comparison, Some(Comparison::Changed { .. })),
        "engine supplies exact byte comparison"
    );
    let record = evidence.records.first().expect("record").record.clone();
    let mut open = first(send(
        &core,
        Event::Open {
            record,
            target: history::OpenTarget::Diff,
        },
    ));
    let response = history::engine::execute(&mut queries, "chain", &open.operation);
    assert!(
        response.is_err(),
        "query adapter cannot claim a native action succeeded"
    );
    let _renders = core.resolve(&mut open, response).expect("native error");
    assert!(
        matches!(core.view().history.open, RequestState::Failed(_)),
        "unavailable host capability is visible"
    );
    let wrong_chain = history::engine::execute(
        &mut queries,
        "other",
        &Query {
            chain: "chain".into(),
            action: QueryAction::Evidence {
                operation: id(1).to_string(),
            },
        },
    );
    assert!(
        wrong_chain.is_err(),
        "explicit chain binding is checked before reading"
    );
}

#[test]
fn adapter_rejects_short_ids_and_exposes_complete_contents_query() {
    let directory = tempfile::tempdir().expect("temporary chain");
    let engine = Engine::open(directory.path()).expect("engine");
    let _admission = engine.append(&message(1, None, b"exact")).expect("message");
    let mut queries = engine.queries().expect("queries");
    assert!(
        history::engine::execute(
            &mut queries,
            "chain",
            &Query {
                chain: "chain".into(),
                action: QueryAction::Evidence {
                    operation: "0101".into()
                }
            }
        )
        .is_err(),
        "prefixes are not persisted as evidence identities"
    );
    let result = history::engine::execute(
        &mut queries,
        "chain",
        &Query {
            chain: "chain".into(),
            action: QueryAction::Evidence {
                operation: id(1).to_string(),
            },
        },
    )
    .expect("exact evidence");
    let evidence = if let QueryResult::Evidence(evidence) = result {
        Some(evidence)
    } else {
        None
    }
    .expect("evidence result");
    assert_eq!(
        evidence.fields.len(),
        2,
        "all message fields, including MIME, are returned"
    );
}
