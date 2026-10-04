use idle_protocol::v1::{
    api::{ApiError, ApiResult, ErrorCode, QueryResult, Response, RetryAdvice},
    events::RecoverySnapshot,
    grants::{GrantScope, GrantStatus},
    identity::Timestamp,
    membership::MembershipStatus,
};

use super::{ready_with, snapshot};
use crate::resources::{
    ResourceAdapterContext, ResourceError, ResourceErrorCode, ResourceOperation,
    ResourceOperationKind, ResourcePermission, ResourceResult, ResourceRetryAdvice,
    ResourceRuntimeInfo, ResourceSnapshot,
    scripted::{ResourceScriptStep, ScriptedResources},
};

fn source() -> RecoverySnapshot {
    let response: Response<QueryResult> =
        serde_json::from_str(idle_protocol::fixtures::STANDALONE_SNAPSHOT).expect("f20 snapshot");
    let value = if let ApiResult::Success(QueryResult::Snapshot(snapshot)) = response.result {
        Some(*snapshot)
    } else {
        None
    };
    value.expect("expected f20 snapshot")
}

fn adapter() -> ResourceAdapterContext {
    ResourceAdapterContext {
        context: snapshot().context,
        now_ms: 1000,
        runtime: ResourceRuntimeInfo::default(),
    }
}

#[test]
fn f20_projection_preserves_resources_epochs_and_independent_grants() {
    let source = source();
    let mut adapter = adapter();
    adapter.runtime = snapshot().runtime;
    let value =
        ResourceSnapshot::from_protocol(&source, &adapter).expect("valid resource projection");
    assert_eq!(
        value.controller.last_epoch, 9_007_199_254_740_995,
        "epochs retain all 64 bits"
    );
    assert_eq!(
        value.grants.len(),
        2,
        "session grants never become resource grants"
    );
    assert!(
        value.grants.iter().all(|grant| !grant
            .permissions
            .contains(&ResourcePermission::InstallModel)),
        "connect/execute cannot imply model management"
    );
    let view = ready_with(value).view().resources;
    assert_eq!(
        view.hosts.first().expect("host").actions,
        [ResourcePermission::ConnectHost],
        "compute permissions stay independent"
    );
    assert!(
        !view
            .models
            .first()
            .expect("model")
            .selectable_for
            .is_empty(),
        "provider grant permits use of the shared local model"
    );
}

#[test]
fn adapter_validates_scope_and_runtime_bindings_and_filters_other_audiences() {
    let mut source = source();
    let adapter = adapter();
    let original = source.clone();
    source.as_of.contributor_id = "different-audience".into();
    assert!(
        ResourceSnapshot::from_protocol(&source, &adapter).is_err(),
        "cursor audience must match"
    );
    source = original.clone();
    source.control.workspace_id = "foreign".into();
    assert!(
        ResourceSnapshot::from_protocol(&source, &adapter).is_err(),
        "controller workspace must match"
    );
    source = original.clone();
    source
        .control
        .lease
        .as_mut()
        .expect("lease")
        .fence
        .workspace_id = "foreign".into();
    assert!(
        ResourceSnapshot::from_protocol(&source, &adapter).is_err(),
        "controller fence workspace must match"
    );
    source = original.clone();
    for grant in &mut source.grants {
        grant.value.grantee = "someone-else".into();
    }
    let value = ResourceSnapshot::from_protocol(&source, &adapter).expect("authorized discovery");
    assert!(
        value.grants.is_empty(),
        "another contributor's grant cannot enable actions"
    );
    let mut adapter = adapter;
    adapter.runtime = snapshot().runtime;
    adapter
        .runtime
        .selections
        .first_mut()
        .expect("selection")
        .target
        .runtime_id = "relocated".into();
    assert!(
        ResourceSnapshot::from_protocol(&original, &adapter).is_err(),
        "runtime model target must match the directory binding"
    );
}

#[test]
fn adapter_preserves_revocation_and_membership_instead_of_hiding_expiry() {
    let mut source = source();
    let mut adapter = adapter();
    adapter.runtime = snapshot().runtime;
    for grant in &mut source.grants {
        if matches!(grant.value.scope, GrantScope::Provider { .. }) {
            grant.value.status = GrantStatus::Revoked {
                revoked_at: Timestamp(999),
                revoked_by: "contributor-alice".into(),
            };
        }
        grant.value.expires_at = Some(Timestamp(1100));
    }
    source
        .memberships
        .iter_mut()
        .find(|record| record.value.contributor_id.0 == adapter.context.contributor_id)
        .expect("member")
        .value
        .status = MembershipStatus::Revoked;
    let value =
        ResourceSnapshot::from_protocol(&source, &adapter).expect("revoked resource snapshot");
    assert!(
        !value.member_active,
        "membership revocation survives projection"
    );
    assert!(
        value.grants.iter().any(|grant| !grant.active),
        "grant revocation survives projection"
    );
    assert!(
        value
            .grants
            .iter()
            .all(|grant| grant.expires_at_ms == Some(1100)),
        "core has expiry deadlines even without an event"
    );
    assert!(
        ready_with(value)
            .view()
            .resources
            .models
            .first()
            .expect("model")
            .selectable_for
            .is_empty(),
        "revoked callers cannot select"
    );
}

#[test]
fn retry_advice_is_preserved_and_scripted_adapter_rejects_unexpected_operations() {
    let error: ResourceError = ApiError {
        code: ErrorCode::StaleControl,
        message: "Controller ownership changed".into(),
        retry: RetryAdvice::SameRequest {
            not_before: Some(Timestamp(1500)),
        },
    }
    .into();
    assert_eq!(
        error.code,
        ResourceErrorCode::Conflict,
        "ownership conflict stays explicit"
    );
    assert_eq!(
        error.retry,
        ResourceRetryAdvice::SameRequest {
            not_before_ms: Some(1500)
        },
        "retry advice is retained"
    );
    let operation = ResourceOperation {
        context: snapshot().context,
        kind: ResourceOperationKind::Snapshot,
    };
    let result = Ok(ResourceResult::Snapshot(Box::new(snapshot())));
    let mut script = ScriptedResources::new([ResourceScriptStep {
        operation: operation.clone(),
        result: result.clone(),
    }]);
    let mut other = operation.clone();
    other.context.contributor_id = "foreign".into();
    assert!(
        script.execute(&other).is_err(),
        "unexpected operation fails closed"
    );
    assert_eq!(script.remaining(), 1, "mismatch retains the scripted step");
    assert_eq!(
        script.execute(&operation),
        result,
        "exact production interface response"
    );
    assert!(
        script.execute(&operation).is_err(),
        "exhausted script never invents success"
    );
}
