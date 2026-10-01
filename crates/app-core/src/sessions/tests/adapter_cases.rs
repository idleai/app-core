use std::{collections::BTreeMap, num::NonZeroU32};

use idle_protocol::v1::{
    WriteCondition,
    api::{ApiResult, Command, Query, QueryResult, Request, Response, RuntimeReport},
    events::RecoverySnapshot,
    grants::{GrantCommand, GrantScope},
};

use super::{attribution, connected, mutation_id, request, select, send, snapshot, submit};
use crate::{
    sessions::{
        Event, SessionAction, SessionAdapterContext, SessionDraft, SessionErrorCode,
        SessionInputState, SessionItemBinding, SessionOperation, SessionPermission, SessionResult,
        SessionSnapshot,
        scripted::{ScriptedSessions, SessionScriptStep},
    },
    workspace::WorkspaceMode,
};

fn protocol_snapshot() -> RecoverySnapshot {
    let response: Response<QueryResult> = serde_json::from_str(include_str!(
        "../../../../idle-protocol/tests/fixtures/managed_snapshot.json"
    ))
    .expect("f20 snapshot fixture");
    match response.result {
        ApiResult::Success(QueryResult::Snapshot(snapshot)) => Some(*snapshot),
        ApiResult::Failure(_)
        | ApiResult::Success(
            QueryResult::CatchUp(_)
            | QueryResult::RequestStatus(_)
            | QueryResult::InputStatus(_)
            | QueryResult::ControlOwnership(_)
            | QueryResult::ControlValidation(_),
        ) => None,
    }
    .expect("snapshot response")
}

#[test]
fn protocol_snapshot_requires_explicit_item_mapping_and_drops_compute_provider_grants() {
    let source = protocol_snapshot();
    let shell = snapshot(WorkspaceMode::Managed, "contributor-bob");
    let adapter = SessionAdapterContext {
        context: shell.context,
        contributor: (&shell.contributor).into(),
        capabilities: shell.capabilities,
        now_ms: shell.now_ms,
    };
    assert!(
        SessionSnapshot::from_protocol(&source, &adapter, &BTreeMap::new()).is_err(),
        "a directory ID never substitutes for an EditChain item"
    );
    let bindings = shell
        .sessions
        .iter()
        .map(|session| (session.id.as_str().into(), session.history.clone()))
        .collect();
    let projected =
        SessionSnapshot::from_protocol(&source, &adapter, &bindings).expect("bound snapshot");
    assert_eq!(
        projected.grants.len(),
        1,
        "compute/provider grants do not grant session participation"
    );
    assert_eq!(
        projected.inputs.len(),
        1,
        "retained runtime facts survive projection"
    );
    assert!(
        matches!(
            projected.inputs.first().map(|update| &update.state),
            Some(SessionInputState::Accepted { .. })
        ),
        "acceptance has no implied delivery order"
    );
    let mut wrong = adapter;
    wrong.context.chain = "other-chain".into();
    assert!(
        SessionSnapshot::from_protocol(&source, &wrong, &bindings).is_err(),
        "chain must match the workspace's explicit binding"
    );
}

#[test]
fn input_effect_preserves_the_exact_f20_request_and_direct_runtime_identity() {
    let expected: Request<Command> = serde_json::from_str(include_str!(
        "../../../../idle-protocol/tests/fixtures/submit_input.json"
    ))
    .expect("f20 input fixture");
    let text = match &expected.body {
        Command::SubmitInput(input) => Some(input.text.clone()),
        Command::PutWorkspace(_)
        | Command::Membership(_)
        | Command::PutSession(_)
        | Command::Resource(_)
        | Command::Grant(_)
        | Command::Control(_) => None,
    }
    .expect("input command");
    let (core, _watch) = connected(snapshot(WorkspaceMode::Managed, "contributor-bob"));
    select(&core);
    let request = request(send(
        &core,
        Event::Submit {
            id: mutation_id("input-42"),
            text,
        },
    ));
    assert_eq!(
        request
            .operation
            .coordination_command()
            .expect("f20 conversion"),
        Some(expected.clone()),
        "all f20 context/payload fields are unchanged"
    );
    let direct = request
        .operation
        .runtime_submission()
        .expect("runtime conversion")
        .expect("runtime input");
    assert_eq!(
        direct.context, expected.context,
        "direct runtime path retains actual contributor and retry deadline"
    );
    assert!(
        direct.coordination_receipt.is_none(),
        "direct runtime path invents no backend receipt"
    );
}

#[test]
fn f20_runtime_fixture_preserves_large_order_and_correlated_identity() {
    let report: RuntimeReport = serde_json::from_str(include_str!(
        "../../../../idle-protocol/tests/fixtures/runtime_completed.json"
    ))
    .expect("f20 runtime fixture");
    let context = snapshot(WorkspaceMode::Managed, "contributor-bob").context;
    let output = SessionResult::from_runtime_report(&report, &context).expect("runtime projection");
    let update = match output {
        SessionResult::Input(update) => Some(update),
        SessionResult::Snapshot(_)
        | SessionResult::Changes(_)
        | SessionResult::SnapshotRequired
        | SessionResult::Created { .. }
        | SessionResult::Acknowledged(_)
        | SessionResult::Unknown => None,
    }
    .expect("runtime input result");
    assert_eq!(
        update.state.delivery().map(|value| value.order),
        Some(9_007_199_254_740_993),
        "runtime order is lossless beyond JavaScript integer precision"
    );
    assert_eq!(
        update.contributor.contributor_id, "contributor-bob",
        "original contributor survives relaying"
    );
    let mut wrong = report;
    wrong.workspace_id = "other-workspace".into();
    assert!(
        SessionResult::from_runtime_report(&wrong, &context).is_err(),
        "outer report scope is validated before projection"
    );
}

#[test]
fn creation_registration_and_sharing_use_typed_f20_commands() {
    let (core, _watch) = connected(snapshot(WorkspaceMode::Managed, "contributor-alice"));
    select(&core);
    let invitation = request(send(
        &core,
        Event::Invite {
            id: mutation_id("invite"),
            grant_id: "grant-new".into(),
            grantee: "contributor-bob".into(),
            permissions: vec![SessionPermission::Observe, SessionPermission::SubmitInput],
            expires_at_ms: None,
        },
    ));
    let command = invitation
        .operation
        .coordination_command()
        .expect("f20 grant conversion")
        .expect("grant command");
    assert!(
        matches!(
            &command.body,
            Command::Grant(GrantCommand::Issue {
                scope: GrantScope::Session { .. },
                ..
            })
        ),
        "sharing is only a session grant"
    );
    let create = request(send(
        &core,
        Event::Create {
            id: mutation_id("create"),
            draft: SessionDraft {
                title: "New runner".into(),
                host_id: "host-shared".into(),
                parent: None,
            },
        },
    ));
    let mut allocated = protocol_snapshot()
        .sessions
        .first()
        .expect("session fixture")
        .value
        .clone();
    allocated.id = "allocated-session".into();
    allocated.title = "New runner".into();
    let registration = create
        .operation
        .registration_request(allocated.clone())
        .expect("runtime allocation registration");
    assert_eq!(
        registration.context,
        (&attribution(&create.operation)).into(),
        "registration preserves creator's retry context"
    );
    assert!(
        matches!(registration.body, Command::PutSession(ref change) if change.expected == WriteCondition::Absent && change.value == allocated),
        "f20 create registration cannot overwrite an existing session"
    );
}

#[test]
fn queries_and_scripts_reject_context_substitution_without_consuming_work() {
    let (core, _watch) = connected(snapshot(WorkspaceMode::Managed, "contributor-bob"));
    select(&core);
    let submission = submit(&core, "query");
    let mut context =
        idle_protocol::v1::identity::RequestContext::from(&attribution(&submission.operation));
    let operation = SessionOperation {
        context: submission.operation.context.clone(),
        action: SessionAction::RequestStatus(attribution(&submission.operation).key()),
    };
    let query = operation
        .coordination_query(context.clone(), NonZeroU32::MIN)
        .expect("status conversion")
        .expect("query");
    assert!(
        matches!(query.body, Query::RequestStatus(ref key) if *key == context.key()),
        "status lookup retains original key"
    );
    context.contributor.contributor_id = "contributor-alice".into();
    assert!(
        operation
            .coordination_query(context, NonZeroU32::MIN)
            .is_err(),
        "query attribution cannot switch to the owner"
    );
    let mut adapter = ScriptedSessions::new([SessionScriptStep {
        operation: operation.clone(),
        result: Ok(SessionResult::Unknown),
    }]);
    assert_eq!(
        adapter
            .execute(&submission.operation)
            .expect_err("unexpected operation")
            .code,
        SessionErrorCode::InvalidRequest,
        "fixture does not automatically fabricate success"
    );
    assert_eq!(
        adapter.remaining(),
        1,
        "unexpected operations do not consume script steps"
    );
    assert_eq!(
        adapter.execute(&operation),
        Ok(SessionResult::Unknown),
        "expected operation still resolves"
    );
}

#[test]
fn malformed_logical_item_snapshot_is_rejected_by_the_reducer() {
    let mut source = snapshot(WorkspaceMode::Managed, "contributor-bob");
    source.sessions.first_mut().expect("session").history = SessionItemBinding {
        chain: "chain-original".into(),
        item: "abc".into(),
    };
    let core = crate::Core::new();
    let mut load = request(send(&core, Event::Connect(source.context.clone())));
    let _effects = core
        .resolve(&mut load, Ok(SessionResult::Snapshot(Box::new(source))))
        .expect("bad snapshot");
    assert!(
        core.view().sessions.sessions.is_empty(),
        "shortened items cannot bind sessions"
    );
}
