use crux_core::bridge::FfiFormat;

use crate::{
    Event, Shell,
    effects::EffectFfi,
    history::{
        self, HistoryPage, OperationDetails, QueryAction, QueryResponse, QueryResult,
        RecordLookupStatus,
    },
    shell::{EffectBatch, PROTOCOL_VERSION, ShellFormat},
};

fn bytes(value: &impl serde::Serialize) -> Vec<u8> {
    let mut bytes = Vec::new();
    ShellFormat::serialize(&mut bytes, value).expect("serialize wire value");
    bytes
}

#[test]
fn history_round_trips_through_shell_and_generated_response_contract() {
    let shell = Shell::new();
    let output = shell
        .process_event(&bytes(&Event::History(history::Event::Connect(
            "chain".into(),
        ))))
        .expect("client action");
    let batch: EffectBatch = ShellFormat::deserialize(&output).expect("effect batch");
    let request = batch
        .requests
        .iter()
        .find(|request| matches!(request.effect, EffectFfi::History(_)))
        .expect("query effect");
    let result = QueryResult::History(HistoryPage {
        observations: vec![super::observation(&super::message(1, None, b"hello\0\xff"))],
        next_after: None,
        scanned: 1,
    });
    let wire = QueryResponse::Ok(result.clone());
    let typed: history::QueryOutput = Ok(result);
    assert_eq!(
        bytes(&wire),
        bytes(&typed),
        "generated named response matches Crux output type"
    );
    assert!(
        shell.handle_response(request.id, &[0]).is_err(),
        "malformed response remains retryable"
    );
    let _effects = shell
        .handle_response(request.id, &bytes(&wire))
        .expect("valid response after malformed bytes");
    let view: crate::ViewModel =
        ShellFormat::deserialize(&shell.view().expect("view bytes")).expect("typed view");
    assert_eq!(
        view.history.items.len(),
        1,
        "foreign host updates shared history state"
    );
    assert_eq!(
        PROTOCOL_VERSION, 4,
        "clients regenerate their payload bindings"
    );
    assert!(
        shell.handle_response(request.id, &bytes(&wire)).is_err(),
        "result is consumed exactly once"
    );
}

#[test]
fn client_cannot_serialize_an_internal_history_completion() {
    let event = Event::History(history::Event::Completed {
        request: 1,
        result: Ok(QueryResult::Opened),
    });
    let mut buffer = Vec::new();
    assert!(
        ShellFormat::serialize(&mut buffer, &event).is_err(),
        "only pending effect continuations admit results"
    );
}

#[test]
fn operation_detail_names_preserve_protocol_v4_binary_layout() {
    assert_eq!(
        PROTOCOL_VERSION, 4,
        "name changes preserve the shell wire version"
    );
    let event = Event::History(history::Event::LoadOperationDetails {
        operation: "op".into(),
        refresh: true,
    });
    assert_eq!(
        bytes(&event),
        b"\x02\0\0\0\x0b\0\0\0\x02\0\0\0\0\0\0\0op\x01",
        "the history route, load action, operation ID and refresh flag keep their encoding"
    );
    assert_eq!(
        bytes(&QueryAction::OperationDetails {
            operation: "op".into()
        }),
        b"\x03\0\0\0\x02\0\0\0\0\0\0\0op",
        "the host lookup keeps its operation discriminant"
    );
    let response = QueryResponse::Ok(QueryResult::OperationDetails(OperationDetails {
        operation: "op".into(),
        status: RecordLookupStatus::Missing,
        observation: None,
        records: Vec::new(),
        fields: Vec::new(),
        comparison: None,
    }));
    let expected = [
        b"\0\0\0\0".as_slice(),  // successful response
        b"\x02\0\0\0",           // operation lookup result
        b"\x02\0\0\0\0\0\0\0op", // operation ID
        b"\x01\0\0\0",           // missing record
        b"\0",                   // absent accepted operation
        &[0; 8],                 // zero raw records
        &[0; 8],                 // zero content fields
        b"\0",                   // absent file comparison
    ]
    .concat();
    assert_eq!(
        bytes(&response),
        expected,
        "result field order and enum discriminants remain stable"
    );
}
