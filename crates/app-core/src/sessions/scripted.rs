//! Explicit development/test adapter for session response scripts.
//!
//! Enabled only by `session-fixtures` (or unit tests). Production hosts resolve the
//! same operations and report unavailable capabilities until real adapters connect.

use std::collections::{BTreeMap, VecDeque};

use idle_protocol::v1::{
    api::{ApiResult, QueryResult, Response},
    identity::SessionId,
};

use crate::workspace::WorkspaceMode;

use super::{
    SessionAdapterContext, SessionCapabilities, SessionCapability, SessionContext,
    SessionContributor, SessionError, SessionGrant, SessionGrantStatus, SessionItemBinding,
    SessionOperation, SessionOutput, SessionPermission, SessionSnapshot, validation,
};

/// One expected production-interface operation and its scripted host response.
#[derive(Clone, Debug)]
pub struct SessionScriptStep {
    /// Exact operation, including attribution and retry context.
    pub operation: SessionOperation,
    /// Scripted runtime/coordination result.
    pub result: SessionOutput,
}

/// Deterministic host adapter; unmatched requests never fabricate a success.
#[derive(Debug, Default)]
pub struct ScriptedSessions {
    steps: VecDeque<SessionScriptStep>,
}

impl ScriptedSessions {
    /// Construct an explicit response script in expected effect order.
    #[must_use]
    pub fn new(steps: impl IntoIterator<Item = SessionScriptStep>) -> Self {
        Self {
            steps: steps.into_iter().collect(),
        }
    }

    /// Execute one expected operation without consuming a mismatched step.
    ///
    /// # Errors
    /// Returns the scripted error, or rejects missing/unexpected operations.
    pub fn execute(&mut self, operation: &SessionOperation) -> SessionOutput {
        if self
            .steps
            .front()
            .is_none_or(|step| step.operation != *operation)
        {
            return Err(validation::invalid(
                "Session operation does not match the next scripted response",
            ));
        }
        self.steps
            .pop_front()
            .ok_or_else(|| validation::invalid("Session response script is exhausted"))?
            .result
    }

    /// Number of responses not yet consumed by the host.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.steps.len()
    }
}

/// Two-contributor fixture using f20's standalone/managed directory contracts.
/// Item mappings below are explicit fixture allocations, independent of all IDs
/// in the coordination records. The initial runtime input list is intentionally empty.
///
/// # Errors
/// Rejects an unknown fixture contributor or a malformed embedded f20 fixture.
pub fn demo_snapshot(
    mode: WorkspaceMode,
    contributor_id: &str,
) -> Result<SessionSnapshot, SessionError> {
    let json = match mode {
        WorkspaceMode::Standalone => idle_protocol::fixtures::STANDALONE_SNAPSHOT,
        WorkspaceMode::Managed => idle_protocol::fixtures::MANAGED_SNAPSHOT,
    };
    let response: Response<QueryResult> = serde_json::from_str(json)
        .map_err(|error| validation::invalid(&format!("Invalid f20 fixture: {error}")))?;
    let ApiResult::Success(QueryResult::Snapshot(mut source)) = response.result else {
        return Err(validation::invalid("Expected f20 fixture snapshot"));
    };
    let contributor = source
        .contributors
        .iter()
        .find(|record| record.value.id.0 == contributor_id)
        .ok_or_else(|| validation::invalid("Unknown fixture contributor"))?;
    let identity =
        contributor.value.identities.first().ok_or_else(|| {
            validation::invalid("Fixture contributor has no authentication identity")
        })?;
    let actor = SessionContributor {
        contributor_id: contributor_id.into(),
        issuer: identity.issuer.clone(),
        subject: identity.subject.clone(),
    };
    source.as_of.contributor_id = contributor_id.into();
    source.inputs.clear();
    let context = SessionContext {
        provider: "scripted-sessions".into(),
        workspace_id: source.workspace.value.id.0.clone(),
        contributor_id: contributor_id.into(),
        chain: source.workspace.value.chain.0.clone(),
        mode,
    };
    let bindings: BTreeMap<SessionId, _> = [
        (
            "session-shared".into(),
            SessionItemBinding {
                chain: context.chain.clone(),
                item: "a".repeat(64),
            },
        ),
        (
            "session-control".into(),
            SessionItemBinding {
                chain: context.chain.clone(),
                item: "b".repeat(64),
            },
        ),
    ]
    .into();
    let adapter = SessionAdapterContext {
        context,
        contributor: (&actor).into(),
        now_ms: 1000,
        capabilities: SessionCapabilities {
            create: SessionCapability::Available,
            input: SessionCapability::Available,
            share: SessionCapability::Available,
        },
    };
    let mut snapshot = SessionSnapshot::from_protocol(&source, &adapter, &bindings)?;
    snapshot.grants.push(SessionGrant {
        id: "grant-session-alice".into(),
        session_id: "session-shared".into(),
        grantee: "contributor-alice".into(),
        granted_by: "contributor-alice".into(),
        permissions: vec![
            SessionPermission::Observe,
            SessionPermission::SubmitInput,
            SessionPermission::Invite,
        ],
        expires_at_ms: None,
        revision: 1,
        status: SessionGrantStatus::Active,
    });
    Ok(snapshot)
}
