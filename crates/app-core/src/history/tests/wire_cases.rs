use crux_core::bridge::FfiFormat;

use crate::{
    Event, Shell,
    effects::EffectFfi,
    history::{self, HistoryPage, QueryResponse, QueryResult},
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
        PROTOCOL_VERSION, 3,
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
