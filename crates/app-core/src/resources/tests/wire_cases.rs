use crux_core::bridge::FfiFormat;

use super::snapshot;
use crate::{
    Event as RootEvent, Shell, ViewModel,
    effects::EffectFfi,
    resources::{Event, ResourceOutput, ResourceResponse, ResourceResult},
    shell::{EffectBatch, PROTOCOL_VERSION, ShellFormat},
};

fn bytes(value: &impl serde::Serialize) -> Vec<u8> {
    let mut result = Vec::new();
    ShellFormat::serialize(&mut result, value).expect("encode resource payload");
    result
}

#[test]
fn resource_shell_round_trips_full_epochs_and_validates_responses_before_consumption() {
    assert_eq!(
        PROTOCOL_VERSION, 13,
        "resource restoration remains available in shell protocol 11"
    );
    let shell = Shell::new();
    let batch: EffectBatch = ShellFormat::deserialize(
        &shell
            .process_event(&bytes(&RootEvent::Resources(Event::Connect(
                snapshot().context,
            ))))
            .expect("connect event"),
    )
    .expect("batch");
    let load = batch
        .requests
        .iter()
        .find(|request| matches!(request.effect, EffectFfi::Resource(_)))
        .expect("resource effect");
    let result = ResourceResult::Snapshot(Box::new(snapshot()));
    let output: ResourceOutput = Ok(result.clone());
    let response = bytes(&ResourceResponse::Ok(result));
    assert_eq!(
        bytes(&output),
        response,
        "generated result matches Rust result layout"
    );
    assert!(
        shell.handle_response(load.id, &[0]).is_err(),
        "truncated response rejected before consuming continuation"
    );
    let mut trailing = response.clone();
    trailing.push(0);
    assert!(
        shell.handle_response(load.id, &trailing).is_err(),
        "trailing bytes rejected before consuming continuation"
    );
    let _effects = shell
        .handle_response(load.id, &response)
        .expect("valid resource response");
    let view: ViewModel =
        ShellFormat::deserialize(&shell.view().expect("view bytes")).expect("typed view");
    assert_eq!(
        view.resources.controller.ownership.last_epoch, 9_007_199_254_740_995,
        "controller epoch remains lossless across native bridge"
    );
    assert_eq!(
        view.resources
            .packages
            .first()
            .expect("package")
            .package_info
            .name,
        "Local coder 🌍",
        "Unicode resource label survives"
    );
    assert!(
        shell.handle_response(load.id, &response).is_err(),
        "a continuation resolves only once"
    );
}

#[test]
fn clients_cannot_serialize_or_deserialize_internal_resource_completions() {
    let completion = Event::Completed {
        token: 1,
        result: Ok(ResourceResult::Unknown),
    };
    assert!(
        serde_json::to_string(&completion).is_err(),
        "internal event cannot serialize"
    );
    assert!(
        serde_json::from_str::<Event>(r#"{"Completed":{"token":1,"result":{"Ok":"Unknown"}}}"#)
            .is_err(),
        "internal result injection is rejected"
    );
    assert!(
        serde_json::from_str::<RootEvent>(
            r#"{"Resources":{"Completed":{"token":1,"result":{"Ok":"Unknown"}}}}"#
        )
        .is_err(),
        "root route cannot forge runtime success"
    );
}
