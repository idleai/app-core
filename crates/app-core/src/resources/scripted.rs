//! Explicit development/test responses behind the production resource interface.
//!
//! Enabled only by `resource-fixtures` or unit tests. Production runtime support
//! remains unavailable until authenticated runtime adapters supply it.

use std::collections::VecDeque;

use idle_protocol::v1::{
    api::{ApiResult, QueryResult, Response},
    grants::{ComputePermission, GrantScope},
};

use super::{
    ControllerOwnership, ControllerPhase, ControllerRuntime, ModelPackage, ModelSelection,
    ModelTarget, ResourceAdapterContext, ResourceCapabilities, ResourceCapability, ResourceContext,
    ResourceError, ResourceOperation, ResourceOutput, ResourceRuntimeInfo, ResourceSnapshot,
    validation,
};
use crate::workspace::WorkspaceMode;

/// One exact expected resource operation and its scripted response.
#[derive(Clone, Debug)]
pub struct ResourceScriptStep {
    /// Production operation, including workspace/audience and original retry data.
    pub operation: ResourceOperation,
    /// Explicit scripted runtime/coordination response.
    pub result: ResourceOutput,
}

/// Development adapter; unexpected operations cannot fabricate success.
#[derive(Debug, Default)]
pub struct ScriptedResources {
    steps: VecDeque<ResourceScriptStep>,
}

impl ScriptedResources {
    /// Construct an explicit script in expected effect order.
    #[must_use]
    pub fn new(steps: impl IntoIterator<Item = ResourceScriptStep>) -> Self {
        Self {
            steps: steps.into_iter().collect(),
        }
    }

    /// Execute one expected operation, retaining mismatched steps for diagnosis.
    ///
    /// # Errors
    /// Returns the scripted error, or rejects an unexpected/missing operation.
    pub fn execute(&mut self, operation: &ResourceOperation) -> ResourceOutput {
        if self
            .steps
            .front()
            .is_none_or(|step| step.operation != *operation)
        {
            return Err(validation::invalid(
                "Resource operation does not match the next scripted response",
            ));
        }
        self.steps
            .pop_front()
            .ok_or_else(|| validation::invalid("Resource script is exhausted"))?
            .result
    }

    /// Number of scripted responses still waiting for an operation.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.steps.len()
    }
}

/// Resource/control fixture built through the published f20 discovery adapter in
/// either coordination mode, with explicitly scripted runtime model support.
///
/// # Errors
/// Rejects malformed embedded f20 data or an inconsistent fixture runtime binding.
pub fn demo_snapshot(mode: WorkspaceMode) -> Result<ResourceSnapshot, ResourceError> {
    let json = match mode {
        WorkspaceMode::Standalone => {
            include_str!("../../../idle-protocol/tests/fixtures/standalone_snapshot.json")
        }
        WorkspaceMode::Managed => {
            include_str!("../../../idle-protocol/tests/fixtures/managed_snapshot.json")
        }
    };
    let response: Response<QueryResult> = serde_json::from_str(json)
        .map_err(|error| validation::invalid(&format!("Invalid resource fixture: {error}")))?;
    let ApiResult::Success(QueryResult::Snapshot(mut source)) = response.result else {
        return Err(validation::invalid("Expected resource fixture snapshot"));
    };
    for record in &mut source.grants {
        if let GrantScope::Compute { permissions, .. } = &mut record.value.scope {
            permissions.push(ComputePermission::ManageModels);
        }
    }
    let controller = ControllerOwnership::from(&source.control);
    let lease = controller
        .lease
        .as_ref()
        .ok_or_else(|| validation::invalid("Fixture controller is unassigned"))?;
    let host = source
        .hosts
        .first()
        .ok_or_else(|| validation::invalid("Fixture host is missing"))?;
    let runtime = ResourceRuntimeInfo {
        capabilities: ResourceCapabilities {
            connect_host: ResourceCapability::Available,
            select_model: ResourceCapability::Available,
            install_model: ResourceCapability::Available,
        },
        selections: vec![
            ModelSelection {
                target: ModelTarget {
                    session_id: "session-shared".into(),
                    host_id: "host-shared".into(),
                    runtime_id: "runtime-evo".into(),
                    control_epoch: None,
                },
                selected: None,
                capability: ResourceCapability::Available,
            },
            ModelSelection {
                target: lease.target.clone(),
                selected: None,
                capability: ResourceCapability::Available,
            },
        ],
        packages: vec![ModelPackage {
            package_id: "fixture/local-coder".into(),
            name: "Local coder 🌍".into(),
            host_id: "host-shared".into(),
            runtime_id: "runtime-evo".into(),
        }],
        controller: Some(ControllerRuntime {
            target: lease.target.clone(),
            phase: ControllerPhase::Running,
            health: (&host.value.health).into(),
        }),
    };
    ResourceSnapshot::from_protocol(
        &source,
        &ResourceAdapterContext {
            context: ResourceContext {
                provider: "scripted-resources".into(),
                workspace_id: source.workspace.value.id.0.clone(),
                contributor_id: source.as_of.contributor_id.0.clone(),
                chain: source.workspace.value.chain.0.clone(),
                mode,
            },
            now_ms: 1000,
            runtime,
        },
    )
}
