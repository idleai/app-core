use crux_core::bridge::FfiFormat;
use idle_protocol::v1::{
    ApiVersion, WriteCondition,
    identity::{ContributorIdentity, ExternalIdentity, Revision},
};

use super::{context, edit, ready, record, save, snapshot};
use crate::{
    Event as RootEvent, Shell, ViewModel,
    configuration::{
        ConfigurationAction, ConfigurationDocument as Document, ConfigurationErrorKind,
        ConfigurationOperation, ConfigurationOutput, ConfigurationResponse, ConfigurationResult,
        ConfigurationSaveState, ConfigurationWrite, Event,
    },
    effects::EffectFfi,
    shell::{EffectBatch, PROTOCOL_VERSION, ShellFormat},
    workspace::WorkspaceMode,
};

fn bytes(value: &impl serde::Serialize) -> Vec<u8> {
    let mut bytes = Vec::new();
    ShellFormat::serialize(&mut bytes, value).expect("encode configuration payload");
    bytes
}

fn identity() -> ContributorIdentity {
    ContributorIdentity {
        contributor_id: "alice".into(),
        authenticated_as: ExternalIdentity {
            issuer: "peer-or-sso".into(),
            subject: "verified-subject".into(),
        },
    }
}

#[test]
fn both_routes_use_the_versioned_coordination_envelope_with_conditional_writes() {
    for mode in [WorkspaceMode::Standalone, WorkspaceMode::Managed] {
        let core = ready(mode);
        edit(
            &core,
            Document::AgentRules,
            r#"{"unknown_rule_field":{"keep":true}}"#,
        );
        let write = save(&core, Document::AgentRules, "envelope");
        let request = write
            .operation
            .write_request(identity())
            .expect("conditional write envelope");
        assert_eq!(
            request.api_version,
            ApiVersion::V1,
            "existing coordination version"
        );
        assert_eq!(
            request.context.key().request_id.0,
            "envelope",
            "original retry identity"
        );
        assert_eq!(
            request.context.contributor,
            identity(),
            "authenticated attribution retained"
        );
        assert_eq!(
            request.body.change.expected,
            WriteCondition::Revision(Revision(3)),
            "atomic revision precondition"
        );
        assert_eq!(
            request.body.document,
            Document::AgentRules,
            "rules have their own revision scope"
        );
        assert!(
            request.control_fence.is_none(),
            "human configuration editing does not assume controller ownership"
        );
        let encoded = serde_json::to_string(&request).expect("coordination JSON");
        let decoded: idle_protocol::v1::api::Request<ConfigurationWrite> =
            serde_json::from_str(&encoded).expect("JSON round trip");
        assert_eq!(
            decoded, request,
            "opaque values and versioned request survive serialization"
        );
        let mut foreign = identity();
        foreign.contributor_id = "someone-else".into();
        assert_eq!(
            write
                .operation
                .write_request(foreign)
                .expect_err("wrong authenticated contributor")
                .kind,
            ConfigurationErrorKind::Unauthenticated,
            "host identity must match selected audience"
        );
    }
}

#[test]
fn configuration_shell_preserves_full_revisions_and_rejects_bad_responses_before_consumption() {
    assert_eq!(
        PROTOCOL_VERSION, 10,
        "configuration extends the binary shell protocol"
    );
    let shell = Shell::new();
    let batch: EffectBatch = ShellFormat::deserialize(
        &shell
            .process_event(&bytes(&RootEvent::Configuration(Event::Connect(context(
                WorkspaceMode::Managed,
            )))))
            .expect("connect"),
    )
    .expect("effects");
    let load = batch.requests.iter().find(|request| matches!(&request.effect, EffectFfi::Configuration(operation) if operation.document == Document::Settings)).expect("configuration request");
    let operation = ConfigurationOperation {
        context: context(WorkspaceMode::Managed),
        document: Document::Settings,
        action: ConfigurationAction::Load,
    };
    let revision = 9_007_199_254_740_995;
    let result = ConfigurationResult::Loaded(snapshot(
        &operation,
        Some(record(revision, r#"{"name":"Workspace 🌍"}"#)),
    ));
    let output: ConfigurationOutput = Ok(result.clone());
    let response = bytes(&ConfigurationResponse::Ok(result));
    assert_eq!(
        bytes(&output),
        response,
        "generated result shares Rust binary layout"
    );
    assert!(
        shell.handle_response(load.id, &[0]).is_err(),
        "truncated data does not consume the continuation"
    );
    let mut trailing = response.clone();
    trailing.push(0);
    assert!(
        shell.handle_response(load.id, &trailing).is_err(),
        "trailing data rejected"
    );
    let _effects = shell
        .handle_response(load.id, &response)
        .expect("valid response");
    let view: ViewModel =
        ShellFormat::deserialize(&shell.view().expect("view bytes")).expect("typed view");
    assert_eq!(
        view.configuration.settings.base_revision,
        Some(revision),
        "large revision remains exact"
    );
    assert_eq!(
        view.configuration.settings.draft.json, r#"{"name":"Workspace 🌍"}"#,
        "Unicode JSON survives native boundary"
    );
    assert_eq!(
        view.configuration.settings.save,
        ConfigurationSaveState::Idle,
        "a load is not a save acknowledgement"
    );
    assert!(
        shell.handle_response(load.id, &response).is_err(),
        "continuation resolves only once"
    );
}

#[test]
fn serialized_clients_cannot_forge_configuration_completions() {
    let operation = ConfigurationOperation {
        context: context(WorkspaceMode::Standalone),
        document: Document::Settings,
        action: ConfigurationAction::Load,
    };
    let completion = Event::Completed {
        token: 1,
        result: Ok(ConfigurationResult::Loaded(snapshot(&operation, None))),
    };
    assert!(
        serde_json::to_string(&completion).is_err(),
        "internal completion cannot serialize"
    );
    assert!(
        serde_json::from_str::<Event>(r#"{"Completed":{"token":1,"result":{}}}"#).is_err(),
        "internal completion cannot deserialize"
    );
    assert!(
        serde_json::from_str::<RootEvent>(
            r#"{"Configuration":{"Completed":{"token":1,"result":{}}}}"#
        )
        .is_err(),
        "root event cannot inject provider results"
    );
}
