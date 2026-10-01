use crux_core::bridge::FfiFormat;
use idle_protocol::v1::projections as protocol;

use super::{context, snapshot};
use crate::{
    Event as RootEvent, Shell, ViewModel,
    effects::EffectFfi,
    projections::{Event, ProjectionOutput, ProjectionResponse, ProjectionSnapshot},
    shell::{EffectBatch, ShellFormat},
};

fn bytes(value: &impl serde::Serialize) -> Vec<u8> {
    let mut bytes = Vec::new();
    ShellFormat::serialize(&mut bytes, value).expect("encode projection payload");
    bytes
}

#[test]
fn shared_json_and_native_results_round_trip_full_references_and_freshness() {
    let source = snapshot();
    let shared = protocol::ProjectionSnapshot::try_from(source.clone()).expect("shared input");
    let json = serde_json::to_value(&shared).expect("shared JSON");
    let shared =
        serde_json::from_value::<protocol::ProjectionSnapshot>(json.clone()).expect("decode JSON");
    assert_eq!(
        ProjectionSnapshot::try_from(shared).expect("shell conversion"),
        source,
        "full input round trip"
    );
    assert_eq!(
        json.pointer("/inputs/0/freshness/generated_at")
            .expect("generation field"),
        "1000",
        "protocol timestamps are lossless strings"
    );
    assert_eq!(
        json.pointer("/inputs/0/total").expect("total field"),
        "1",
        "protocol counts are lossless strings"
    );
    let output: ProjectionOutput = Ok(source.clone());
    assert_eq!(
        bytes(&output),
        bytes(&ProjectionResponse::Ok(source)),
        "named shell result matches Crux output"
    );
}

#[test]
fn shell_checks_results_before_consuming_and_hides_internal_completions() {
    let shell = Shell::new();
    let batch: EffectBatch = ShellFormat::deserialize(
        &shell
            .process_event(&bytes(&RootEvent::Projections(Event::Connect(context()))))
            .expect("event"),
    )
    .expect("batch");
    let operation = batch
        .requests
        .iter()
        .find(|request| matches!(request.effect, EffectFfi::Projection(_)))
        .expect("projection effect");
    assert!(
        shell.handle_response(operation.id, &[0]).is_err(),
        "malformed result leaves continuation available"
    );
    let result = bytes(&ProjectionResponse::Ok(snapshot()));
    let _effects = shell
        .handle_response(operation.id, &result)
        .expect("correct result");
    let view: ViewModel =
        ShellFormat::deserialize(&shell.view().expect("view")).expect("typed view");
    assert_eq!(
        view.projections.tasks.total,
        Some(1),
        "native response reaches view"
    );
    assert!(
        shell.handle_response(operation.id, &result).is_err(),
        "one-shot continuation cannot be replayed"
    );
    let event = RootEvent::Projections(Event::Completed {
        token: 1,
        result: Ok(snapshot()),
    });
    assert!(
        ShellFormat::serialize(&mut Vec::new(), &event).is_err(),
        "client cannot serialize a completion"
    );
    assert!(
        shell.process_event(&[6, 0, 0, 0, 9, 0, 0, 0]).is_err(),
        "internal completion discriminant cannot be decoded"
    );
}
