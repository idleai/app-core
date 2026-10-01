use crux_core::bridge::FfiFormat;

use super::snapshot;
use crate::{
    Event as RootEvent, Shell, ViewModel,
    effects::EffectFfi,
    sessions::{Event, SessionOutput, SessionResponse, SessionResult},
    shell::{EffectBatch, ShellFormat},
    workspace::WorkspaceMode,
};

fn bytes(value: &impl serde::Serialize) -> Vec<u8> {
    let mut bytes = Vec::new();
    ShellFormat::serialize(&mut bytes, value).expect("encode session payload");
    bytes
}

#[test]
fn session_shell_payloads_round_trip_and_malformed_responses_leave_continuations_pending() {
    let shell = Shell::new();
    let source = snapshot(WorkspaceMode::Managed, "contributor-bob");
    let effects = shell
        .process_event(&bytes(&RootEvent::Sessions(Event::Connect(
            source.context.clone(),
        ))))
        .expect("session event");
    let batch: EffectBatch = ShellFormat::deserialize(&effects).expect("effect batch");
    let operation = batch
        .requests
        .iter()
        .find(|request| matches!(request.effect, EffectFfi::Session(_)))
        .expect("session effect");
    let result = SessionResult::Snapshot(Box::new(source));
    let output: SessionOutput = Ok(result.clone());
    let wire = SessionResponse::Ok(Box::new(result));
    assert_eq!(
        bytes(&output),
        bytes(&wire),
        "named generated response matches typed Crux result"
    );
    assert!(
        shell.handle_response(operation.id, &[0]).is_err(),
        "malformed response is rejected before consuming continuation"
    );
    let _effects = shell
        .handle_response(operation.id, &bytes(&wire))
        .expect("corrected session response");
    let view: ViewModel =
        ShellFormat::deserialize(&shell.view().expect("view bytes")).expect("typed session view");
    assert_eq!(
        view.sessions.sessions.len(),
        1,
        "native response updates typed invited session view"
    );
    assert!(
        shell.handle_response(operation.id, &bytes(&wire)).is_err(),
        "one-shot response cannot be replayed"
    );
}

#[test]
fn shell_rejects_forged_runtime_completions() {
    let event = RootEvent::Sessions(Event::Completed {
        token: 1,
        result: Ok(SessionResult::Unknown),
    });
    let mut output = Vec::new();
    assert!(
        ShellFormat::serialize(&mut output, &event).is_err(),
        "client cannot serialize internal runtime facts"
    );
    assert!(
        Shell::new()
            .process_event(&[5, 0, 0, 0, 12, 0, 0, 0])
            .is_err(),
        "internal completion discriminant cannot be decoded"
    );
}
