use crux_core::bridge::FfiFormat;

use super::info;
use crate::{
    Event as RootEvent, Shell, ViewModel,
    effects::EffectFfi,
    shell::{EffectBatch, ShellFormat},
    workspace::{
        Event, WorkspaceMode, WorkspaceOperation, WorkspaceOutput, WorkspaceResponse,
        WorkspaceResult,
    },
};

fn bytes(value: &impl serde::Serialize) -> Vec<u8> {
    let mut bytes = Vec::new();
    ShellFormat::serialize(&mut bytes, value).expect("serialize workspace wire payload");
    bytes
}

#[test]
fn workspace_effects_and_views_round_trip_through_native_shell_contract() {
    let shell = Shell::new();
    let effects = shell
        .process_event(&bytes(&RootEvent::Workspace(Event::Load)))
        .expect("load");
    let batch: EffectBatch = ShellFormat::deserialize(&effects).expect("effect batch");
    let request = batch
        .requests
        .iter()
        .find(|request| matches!(request.effect, EffectFfi::Workspace(_)))
        .expect("workspace effect");
    assert_eq!(
        request.effect,
        EffectFfi::Workspace(Box::new(WorkspaceOperation::List)),
        "generated workspace operation"
    );
    let info = info("local", WorkspaceMode::Standalone);
    let result = WorkspaceResult::Directory(vec![info.clone()]);
    let typed: WorkspaceOutput = Ok(result.clone());
    let named = WorkspaceResponse::Ok(result);
    assert_eq!(
        bytes(&typed),
        bytes(&named),
        "generated result enum matches Crux's result"
    );
    assert!(
        shell.handle_response(request.id, &[0]).is_err(),
        "malformed bytes do not consume continuation"
    );
    let _effects = shell
        .handle_response(request.id, &bytes(&named))
        .expect("corrected result");
    let view: ViewModel =
        ShellFormat::deserialize(&shell.view().expect("view bytes")).expect("typed view");
    assert_eq!(
        view.workspace.workspaces,
        [info],
        "wire response updates typed navigation view"
    );
    assert!(
        shell.handle_response(request.id, &bytes(&named)).is_err(),
        "one-shot completion cannot be replayed"
    );
    let output = shell
        .process_event(&bytes(&RootEvent::Workspace(Event::SelectWorkspace(
            "local".into(),
        ))))
        .expect("select");
    let batch: EffectBatch = ShellFormat::deserialize(&output).expect("selection effects");
    assert!(batch.requests.iter().any(|request| matches!(&request.effect, EffectFfi::History(query) if query.chain == "chain-local")), "selection emits a chain-only engine operation through shell");
}

#[test]
fn client_cannot_forge_a_workspace_completion() {
    let event = RootEvent::Workspace(Event::Completed {
        request: 1,
        result: Ok(WorkspaceResult::Directory(Vec::new())),
    });
    let mut output = Vec::new();
    assert!(
        ShellFormat::serialize(&mut output, &event).is_err(),
        "results require pending host continuations"
    );
    let shell = Shell::new();
    assert!(
        shell.process_event(&[3, 0, 0, 0, 8, 0, 0, 0]).is_err(),
        "internal completion discriminant cannot be decoded"
    );
}
