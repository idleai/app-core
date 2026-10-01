use editchain_core::{
    OpKind, Payload,
    activity::{Field, Kind, MessageKind, Operation},
};
use editchain_engine::{
    Engine,
    queries::{ChainQueries, ContentField, ContentValue},
};
use idle_protocol::v1::projections::ProjectionInput;

use super::{context, request, send};
use crate::{
    Core,
    module::EffectError,
    projections::{
        Event, FreshnessStatus, ProjectionAvailability, ProjectionKind, ProjectionQuery,
        ProjectionSelection,
        engine::{self, ProjectionMapper, ProjectionRead, UnavailableMapper},
        fixtures::{self, FixtureMapper, id},
    },
};

fn query(limit: u32) -> ProjectionQuery {
    ProjectionQuery {
        context: context(),
        limit,
    }
}

fn note_with_blob(source: &Engine) -> (editchain_core::Op, Vec<u8>) {
    let note = fixtures::operations()
        .expect("fixtures")
        .into_iter()
        .find(|op| op.id == id(10))
        .expect("task note");
    let mut activity = Operation::view(&note).expect("schema-three note");
    let payload = if let Kind::Note(payload) = &mut activity.kind {
        Some(payload)
    } else {
        None
    }
    .expect("note payload");
    let bytes = if let Payload::Inline(bytes) = &payload.content {
        Some(bytes.clone())
    } else {
        None
    }
    .expect("inline fixture body");
    payload.content = Payload::Blob(source.store_blob(&bytes).expect("fixture blob"));
    (activity.into_op().expect("valid blob note"), bytes)
}

#[derive(Debug)]
struct InspectMapper;

impl ProjectionMapper for InspectMapper {
    fn inputs(
        &self,
        queries: &ChainQueries,
        read: &ProjectionRead,
    ) -> Result<Vec<ProjectionInput>, EffectError> {
        verify_read(read);
        FixtureMapper.inputs(queries, read)
    }
}

fn verify_read(read: &ProjectionRead) {
    let link = read
        .records
        .iter()
        .find(|record| record.entry.operation.id == id(50))
        .expect("link");
    assert_eq!(
        link.entry
            .operation
            .parent_ids()
            .copied()
            .collect::<Vec<_>>(),
        vec![id(10), id(20), id(30)],
        "all causal parents retained"
    );
    let summary = read
        .records
        .iter()
        .find(|record| record.entry.operation.id == id(60))
        .expect("summary");
    let activity = Operation::view(&summary.entry.operation).expect("schema-three summary");
    assert_ne!(
        activity.author,
        Some(activity.recorder),
        "author remains distinct from recorder"
    );
    assert!(
        matches!(&activity.kind, Kind::Message(message) if message.category == MessageKind::Summary && message.coverage.as_ref().is_some_and(|coverage| coverage.operations == vec![id(10), id(20), id(30), id(40)])),
        "exact recorded coverage is retained"
    );
    assert!(
        summary.fields.iter().any(|field| field.field
            == ContentField::Record(Field::MessageBlock(0))
            && field.value
                == ContentValue::Available(
                    b"Checks are pending. A provider choice is needed.\n".to_vec()
                )),
        "engine returns exact summary bytes"
    );
}

#[test]
fn engine_queries_feed_all_destinations_without_production_payload_inference() {
    let directory = tempfile::tempdir().expect("chain");
    let engine = Engine::open(directory.path()).expect("engine");
    fixtures::seed(&engine).expect("validated fixture writes");
    let mut queries = engine.queries().expect("queries");
    let input = engine::execute(&mut queries, "chain", &query(100), &InspectMapper)
        .expect("mapped fixtures");
    for kind in ProjectionKind::ALL {
        let list = input
            .inputs
            .iter()
            .find(|input| input.kind == kind)
            .expect("destination");
        assert_eq!(
            list.availability,
            ProjectionAvailability::Complete,
            "complete fixture scope"
        );
        assert_eq!(
            list.total,
            Some(if kind == ProjectionKind::Activity {
                6
            } else {
                1
            }),
            "counts are supplied explicitly"
        );
        for row in &list.rows {
            let source = row.sources.first().expect("source");
            assert_eq!(
                source.observation.as_ref().expect("observation").len(),
                64,
                "full observation identity"
            );
            assert_eq!(
                source.item.as_ref().expect("item").len(),
                64,
                "full item identity"
            );
            assert_ne!(
                source.observation, source.item,
                "physical and logical identities differ"
            );
            assert_eq!(
                source.record_hash.as_ref().expect("digest").len(),
                64,
                "exact stored encoding reference"
            );
        }
    }
    let activity = input
        .inputs
        .iter()
        .find(|input| input.kind == ProjectionKind::Activity)
        .expect("activity");
    let link = activity
        .rows
        .iter()
        .find(|row| row.key == id(50).to_string())
        .expect("link row");
    for parent in [10, 20, 30, 60] {
        assert!(
            link.related
                .iter()
                .any(|reference| reference.observation.as_deref()
                    == Some(id(parent).to_string().as_str())),
            "parents and explicit Link destination remain addressable"
        );
    }
    let default = engine::execute(&mut queries, "chain", &query(100), &UnavailableMapper)
        .expect("production default");
    for list in default
        .inputs
        .iter()
        .filter(|input| input.kind != ProjectionKind::Activity)
    {
        assert_eq!(
            list.availability,
            ProjectionAvailability::Unavailable,
            "production never activates fixture mappings"
        );
        assert!(
            list.total.is_none(),
            "unavailable is not a known empty board"
        );
    }
    assert!(
        engine::execute(&mut queries, "wrong", &query(100), &FixtureMapper).is_err(),
        "chain binding enforced"
    );
    assert!(
        engine::execute(&mut queries, "chain", &query(0), &FixtureMapper).is_err(),
        "invalid bounds rejected"
    );
}

#[test]
fn bounded_reads_are_partial_and_never_claim_a_durable_checkpoint() {
    let directory = tempfile::tempdir().expect("chain");
    let engine = Engine::open(directory.path()).expect("engine");
    fixtures::seed(&engine).expect("fixtures");
    let mut queries = engine.queries().expect("queries");
    let input =
        engine::execute(&mut queries, "chain", &query(1), &FixtureMapper).expect("bounded read");
    let activity = input.inputs.first().expect("activity");
    assert_eq!(
        activity.availability,
        ProjectionAvailability::Partial,
        "candidate limit is explicit"
    );
    assert!(
        activity.total.is_none(),
        "page length cannot establish a total"
    );
    assert_eq!(
        activity.freshness.status,
        FreshnessStatus::Unknown,
        "index refresh cannot establish controller freshness"
    );
    assert!(
        activity.freshness.checkpoint.is_none(),
        "operation page cursor never becomes a checkpoint"
    );
    assert!(
        !activity.gaps.is_empty(),
        "bounded result explains missing scope"
    );
}

#[test]
fn late_blobs_rebuild_results_without_changing_record_or_item_references() {
    let directory = tempfile::tempdir().expect("chain");
    let blob_directory = tempfile::tempdir().expect("blob source");
    let engine = Engine::open(directory.path()).expect("engine");
    let blob_source = Engine::open(blob_directory.path()).expect("blob engine");
    let (note, bytes) = note_with_blob(&blob_source);
    let _admission = engine.append(&note).expect("note before content");
    for op in fixtures::operations()
        .expect("fixtures")
        .into_iter()
        .filter(|op| op.id != note.id)
    {
        let _admission = engine.append(&op).expect("other records");
    }
    let mut queries = engine.queries().expect("queries");
    let first =
        engine::execute(&mut queries, "chain", &query(100), &FixtureMapper).expect("partial input");
    let tasks = first
        .inputs
        .iter()
        .find(|input| input.kind == ProjectionKind::Task)
        .expect("tasks");
    assert!(tasks.rows.is_empty(), "missing payload is not interpreted");
    assert_eq!(
        tasks.availability,
        ProjectionAvailability::Partial,
        "missing content is not an empty complete board"
    );
    let source = first
        .inputs
        .first()
        .expect("activity")
        .rows
        .iter()
        .find(|row| row.key == id(10).to_string())
        .expect("missing-content row")
        .sources
        .clone();
    let _blob = engine.store_blob(&bytes).expect("late content");
    let second = engine::execute(&mut queries, "chain", &query(100), &FixtureMapper)
        .expect("late blob refresh");
    let tasks = second
        .inputs
        .iter()
        .find(|input| input.kind == ProjectionKind::Task)
        .expect("tasks");
    assert_eq!(
        tasks.rows.first().expect("now available").sources,
        source,
        "content availability does not change recorded identities"
    );
    assert_eq!(
        tasks.availability,
        ProjectionAvailability::Complete,
        "gap clears when content arrives"
    );
    let _stats = queries.rebuild().expect("rebuild index");
    assert_eq!(
        engine::execute(&mut queries, "chain", &query(100), &FixtureMapper).expect("rebuilt input"),
        second,
        "indexes remain rebuildable"
    );
}

#[test]
fn duplicate_delivery_lower_ids_and_conflict_retractions_replace_visible_state() {
    let directory = tempfile::tempdir().expect("chain");
    let engine = Engine::open(directory.path()).expect("engine");
    fixtures::seed(&engine).expect("fixtures");
    let mut queries = engine.queries().expect("queries");
    let core = Core::new();
    let mut load = request(send(&core, Event::Connect(context())));
    let result = engine::execute(&mut queries, "chain", &load.operation, &FixtureMapper);
    let _effects = core.resolve(&mut load, result).expect("initial input");
    let selection = ProjectionSelection {
        kind: ProjectionKind::Task,
        key: "task/checks".into(),
    };
    let _effects = send(&core, Event::Select(Some(selection.clone())));
    fixtures::seed(&engine).expect("duplicate delivery");
    let mut late = fixtures::operations()
        .expect("fixtures")
        .into_iter()
        .find(|op| op.id == id(60))
        .expect("summary");
    if let OpKind::Activity(record) = &mut late.kind {
        record.id = id(1);
    }
    late.id = id(1);
    let _admission = engine.append(&late).expect("late lower ID");
    let mut load = request(send(&core, Event::Refresh));
    let result = engine::execute(&mut queries, "chain", &load.operation, &FixtureMapper);
    let _effects = core.resolve(&mut load, result).expect("late snapshot");
    assert_eq!(
        core.view().projections.activity.loaded_count,
        7,
        "late lower ID appears once after duplicate delivery"
    );
    assert_eq!(
        core.view().projections.selected,
        Some(selection),
        "same-context selection survives replacement"
    );
    let mut conflict = fixtures::operations()
        .expect("fixtures")
        .into_iter()
        .find(|op| op.id == id(10))
        .expect("task");
    if let OpKind::Activity(record) = &mut conflict.kind
        && let Kind::Note(note) = &mut record.kind
    {
        note.content = Payload::Inline(b"conflicting representation".to_vec());
    }
    let _admission = engine.append(&conflict).expect("retained conflict");
    let mut load = request(send(&core, Event::Refresh));
    let result = engine::execute(&mut queries, "chain", &load.operation, &FixtureMapper);
    let _effects = core.resolve(&mut load, result).expect("conflict refresh");
    let view = core.view().projections;
    assert!(
        view.tasks.rows.is_empty(),
        "quarantined facts are retracted"
    );
    assert!(
        view.selected.is_none(),
        "retracted row cannot remain selected"
    );
    assert_eq!(
        view.tasks.availability,
        ProjectionAvailability::Partial,
        "conflict stays explicit"
    );
    assert!(
        view.activity
            .gaps
            .iter()
            .any(|gap| gap
                .reference
                .as_ref()
                .is_some_and(|reference| reference.observation.as_deref()
                    == Some(id(10).to_string().as_str()))),
        "conflicted summary/Link endpoint retains its exact address"
    );
}
