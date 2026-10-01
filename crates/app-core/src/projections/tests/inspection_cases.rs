use editchain_core::{OpKind, Payload, activity::Kind};
use editchain_engine::{Engine, queries::ChainQueries};

use super::{context, request, send};
use crate::{
    Core, Effect, Event as RootEvent, history,
    projections::{
        Event, ProjectionKind, ProjectionReference, ProjectionSelection, engine,
        fixtures::{self, FixtureMapper, id},
    },
};

fn history_results(core: &Core, queries: &mut ChainQueries, mut effects: Vec<Effect>) {
    while let Some(effect) = effects.pop() {
        if let Effect::History(mut pending) = effect {
            let result = history::engine::execute(queries, "chain", &pending.operation);
            effects.extend(core.resolve(&mut *pending, result).expect("history result"));
        }
    }
}

fn inspect(core: &Core, reference: ProjectionReference) -> Vec<Effect> {
    send(
        core,
        Event::Inspect {
            selection: ProjectionSelection {
                kind: ProjectionKind::Task,
                key: "task/checks".into(),
            },
            reference,
        },
    )
}

fn selected(core: &Core, observation: u8, item: Option<u8>) {
    assert_eq!(
        core.view().history.selected,
        history::Selected {
            observation: Some(id(observation).to_string()),
            item: item.map(|number| id(number).to_string()),
        },
        "drill-down selects the exact observation and only a known item"
    );
}

fn detail_status(core: &Core, expected: &history::RecordLookupStatus) {
    assert!(core.view().history.operation_details.iter().any(|details| {
        details.operation == id(60).to_string()
            && matches!(&details.state, history::OperationDetailsState::Ready(result) if result.status == *expected)
    }), "selected target has the expected lookup status");
}

#[test]
fn observation_only_links_select_missing_late_cached_and_conflicted_targets() {
    let directory = tempfile::tempdir().expect("chain");
    let engine = Engine::open(directory.path()).expect("engine");
    let records = fixtures::operations().expect("fixtures");
    let note = records
        .iter()
        .find(|record| record.id == id(10))
        .expect("task note");
    let summary = records
        .iter()
        .find(|record| record.id == id(60))
        .expect("summary");
    let _admission = engine.append(note).expect("note before its target");
    let mut queries = engine.queries().expect("queries");
    let core = Core::new();
    let mut load = request(send(&core, Event::Connect(context())));
    let snapshot = engine::execute(&mut queries, "chain", &load.operation, &FixtureMapper)
        .expect("projection snapshot");
    let row = snapshot
        .inputs
        .iter()
        .find(|input| input.kind == ProjectionKind::Task)
        .expect("tasks")
        .rows
        .first()
        .expect("task");
    let source = row.sources.first().expect("source").clone();
    let missing = row
        .related
        .iter()
        .find(|reference| reference.observation.as_deref() == Some(id(60).to_string().as_str()))
        .expect("missing summary target")
        .clone();
    assert!(
        missing.item.is_none(),
        "unavailable targets have no guessed logical item"
    );
    let _effects = core
        .resolve(&mut load, Ok(snapshot))
        .expect("projection input");

    history_results(&core, &mut queries, inspect(&core, source.clone()));
    selected(&core, 10, Some(110));
    let effects = inspect(&core, missing.clone());
    selected(&core, 60, None);
    history_results(&core, &mut queries, effects);
    selected(&core, 60, None);
    detail_status(&core, &history::RecordLookupStatus::Missing);

    let _admission = engine.append(summary).expect("late target");
    let _pending = core.process_event(RootEvent::History(history::Event::Refresh));
    history_results(&core, &mut queries, inspect(&core, missing.clone()));
    selected(&core, 60, Some(160));
    detail_status(&core, &history::RecordLookupStatus::Found);

    history_results(&core, &mut queries, inspect(&core, source));
    selected(&core, 10, Some(110));
    let effects = inspect(&core, missing.clone());
    selected(&core, 60, Some(160));
    assert!(effects.iter().any(|effect| matches!(effect, Effect::History(request) if matches!(&request.operation.action, history::QueryAction::OperationDetails { operation } if *operation == id(60).to_string()))), "cached observations still refresh their exact details");
    history_results(&core, &mut queries, effects);

    let mut conflict = summary.clone();
    if let OpKind::Activity(record) = &mut conflict.kind
        && let Kind::Message(message) = &mut record.kind
    {
        message.blocks.first_mut().expect("summary block").content =
            Payload::Inline(b"Conflicting summary".to_vec());
    }
    let _admission = engine.append(&conflict).expect("quarantined target");
    history_results(&core, &mut queries, inspect(&core, missing));
    selected(&core, 60, Some(160));
    detail_status(&core, &history::RecordLookupStatus::Conflicted);

    let effects = core.process_event(RootEvent::History(history::Event::Select(
        history::Selected {
            observation: Some(id(60).to_string()),
            item: Some(id(110).to_string()),
        },
    )));
    assert!(
        effects.is_empty(),
        "an explicitly mismatched item is still rejected"
    );
    selected(&core, 60, Some(160));
}
