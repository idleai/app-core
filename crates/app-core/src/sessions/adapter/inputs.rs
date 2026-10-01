//! Runtime facts and backend acknowledgements retain separate acknowledgement stages.

use idle_protocol::v1::{
    api::{ApiResult, BackendReceipt, CommandResult, Response, RuntimeReport},
    sessions::{Completion, Delivery, InputRef, RuntimeInputState, RuntimeInputUpdate},
};

use super::super::{
    SessionAcknowledgement, SessionCompletion, SessionContext, SessionDelivery, SessionError,
    SessionInputRef, SessionInputState, SessionInputUpdate, SessionReceipt, SessionResult,
    validation,
};

impl From<&InputRef> for SessionInputRef {
    fn from(value: &InputRef) -> Self {
        Self {
            session_id: value.session_id.0.clone(),
            request: (&value.request).into(),
        }
    }
}

impl From<&SessionInputRef> for InputRef {
    fn from(value: &SessionInputRef) -> Self {
        Self {
            session_id: value.session_id.as_str().into(),
            request: (&value.request).into(),
        }
    }
}

impl From<&BackendReceipt> for SessionReceipt {
    fn from(value: &BackendReceipt) -> Self {
        Self {
            request: (&value.request).into(),
            received_at_ms: value.received_at.0,
            retry_until_ms: value.retry_until.0,
        }
    }
}

impl From<&Delivery> for SessionDelivery {
    fn from(value: &Delivery) -> Self {
        Self {
            accepted_at_ms: value.accepted_at.0,
            order: value.order.0,
            ordered_at_ms: value.ordered_at.0,
        }
    }
}

impl From<&RuntimeInputUpdate> for SessionInputUpdate {
    fn from(value: &RuntimeInputUpdate) -> Self {
        let state = match &value.state {
            RuntimeInputState::Rejected { rejected_at, error } => SessionInputState::Rejected {
                at_ms: rejected_at.0,
                error: error.clone().into(),
            },
            RuntimeInputState::Accepted { accepted_at } => SessionInputState::Accepted {
                at_ms: accepted_at.0,
            },
            RuntimeInputState::Ordered { delivery } => SessionInputState::Ordered(delivery.into()),
            RuntimeInputState::Running {
                delivery,
                started_at,
            } => SessionInputState::Running {
                delivery: delivery.into(),
                started_at_ms: started_at.0,
            },
            RuntimeInputState::Completed {
                delivery,
                completed_at,
                outcome,
            } => SessionInputState::Completed {
                delivery: delivery.into(),
                completed_at_ms: completed_at.0,
                outcome: match outcome {
                    Completion::Succeeded => SessionCompletion::Succeeded,
                    Completion::Failed(error) => SessionCompletion::Failed(error.clone().into()),
                    Completion::Cancelled => SessionCompletion::Cancelled,
                },
            },
        };
        Self {
            input: (&value.input).into(),
            contributor: (&value.contributor).into(),
            runtime_id: value.runtime_id.0.clone(),
            revision: value.revision.0,
            state,
        }
    }
}

impl SessionResult {
    /// Project a direct runtime acknowledgement without a backend receipt.
    ///
    /// # Errors
    /// Rejects mismatched response/input keys and returns explicit runtime API errors.
    pub fn from_runtime_response(
        value: Response<RuntimeInputUpdate>,
    ) -> Result<Self, SessionError> {
        match value.result {
            ApiResult::Failure(error) => Err(error.into()),
            ApiResult::Success(update) => {
                if update.input.request != value.request {
                    return Err(validation::invalid(
                        "Runtime response and original input keys differ",
                    ));
                }
                Ok(Self::Input((&update).into()))
            }
        }
    }

    /// Project a correlated f20 command result without inventing runtime success.
    ///
    /// # Errors
    /// Rejects mismatched inner/outer keys, unrelated Control results and API errors.
    pub fn from_command_response(value: Response<CommandResult>) -> Result<Self, SessionError> {
        let ack = match value.result {
            ApiResult::Failure(error) => return Err(error.into()),
            ApiResult::Success(CommandResult::Received(receipt)) => {
                if receipt.request != value.request {
                    return Err(validation::invalid(
                        "Response and backend receipt request keys differ",
                    ));
                }
                SessionAcknowledgement::Received((&receipt).into())
            }
            ApiResult::Success(CommandResult::Committed(commit)) => {
                if commit.receipt.request != value.request {
                    return Err(validation::invalid(
                        "Response and metadata commit request keys differ",
                    ));
                }
                SessionAcknowledgement::Committed {
                    receipt: (&commit.receipt).into(),
                    committed_at_ms: commit.committed_at.0,
                    through: (&commit.through).into(),
                }
            }
            ApiResult::Success(CommandResult::Control { .. }) => {
                return Err(validation::invalid(
                    "Control ownership results are not session acknowledgements",
                ));
            }
        };
        Ok(Self::Acknowledged(ack))
    }

    /// Project an authenticated runtime report, retaining its original contributor.
    ///
    /// # Errors
    /// Rejects a report whose outer or input scope differs from the current workspace.
    /// The reducer additionally checks its runtime binding and input revision.
    pub fn from_runtime_report(
        value: &RuntimeReport,
        context: &SessionContext,
    ) -> Result<Self, SessionError> {
        if value.workspace_id.0 != context.workspace_id
            || value.workspace_id != value.update.input.request.workspace_id
        {
            return Err(validation::invalid(
                "Runtime report has mismatched workspace scopes",
            ));
        }
        Ok(Self::Input((&value.update).into()))
    }
}
