//! Production request construction preserves f20 attribution and retry context.

use std::num::NonZeroU32;

use idle_protocol::v1::{
    ApiVersion, Change, WriteCondition,
    api::{Command, Query, Request, RuntimeSubmission},
    grants::{GrantCommand, GrantScope, SessionPermission as ProtocolPermission},
    identity::{RequestContext, Revision, Timestamp},
    sessions::{Session, SessionKind, SubmitInput},
};

use super::super::{
    SessionAction, SessionError, SessionMutation, SessionOperation, SessionPermission, validation,
};

impl From<ProtocolPermission> for SessionPermission {
    fn from(value: ProtocolPermission) -> Self {
        match value {
            ProtocolPermission::Observe => Self::Observe,
            ProtocolPermission::SubmitInput => Self::SubmitInput,
            ProtocolPermission::Invite => Self::Invite,
        }
    }
}

impl From<SessionPermission> for ProtocolPermission {
    fn from(value: SessionPermission) -> Self {
        match value {
            SessionPermission::Observe => Self::Observe,
            SessionPermission::SubmitInput => Self::SubmitInput,
            SessionPermission::Invite => Self::Invite,
        }
    }
}

impl SessionOperation {
    /// Build an f20 coordination mutation without changing attribution or retry IDs.
    /// Runtime creation has no directory-only substitute and returns `None` here.
    ///
    /// # Errors
    /// Rejects attribution that does not match the operation's authenticated scope.
    pub fn coordination_command(&self) -> Result<Option<Request<Command>>, SessionError> {
        let SessionAction::Mutate { request, mutation } = &self.action else {
            return Ok(None);
        };
        let context = RequestContext::from(request);
        self.check_request_context(&context)?;
        let body = match mutation {
            SessionMutation::Create(_) => return Ok(None),
            SessionMutation::Submit { session_id, text } => Command::SubmitInput(SubmitInput {
                session_id: session_id.as_str().into(),
                text: text.clone(),
            }),
            SessionMutation::Invite {
                session_id,
                grant_id,
                grantee,
                permissions,
                expires_at_ms,
            } => Command::Grant(GrantCommand::Issue {
                grant_id: grant_id.as_str().into(),
                grantee: grantee.as_str().into(),
                scope: GrantScope::Session {
                    session_id: session_id.as_str().into(),
                    permissions: permissions.iter().copied().map(Into::into).collect(),
                },
                expires_at: expires_at_ms.map(Timestamp),
            }),
            SessionMutation::Revoke {
                grant_id,
                expected_revision,
                ..
            } => Command::Grant(GrantCommand::Revoke {
                grant_id: grant_id.as_str().into(),
                expected_revision: Revision(*expected_revision),
            }),
        };
        Ok(Some(Request {
            api_version: ApiVersion::V1,
            context,
            control_fence: None,
            body,
        }))
    }

    /// Build a direct f20 runtime input submission with the original human identity.
    /// Forwarding a coordination-received input is the coordination adapter's job.
    ///
    /// # Errors
    /// Rejects attribution that does not match the operation's scope.
    pub fn runtime_submission(&self) -> Result<Option<RuntimeSubmission>, SessionError> {
        let Some(request) = self.coordination_command()? else {
            return Ok(None);
        };
        let Command::SubmitInput(body) = request.body else {
            return Ok(None);
        };
        Ok(Some(RuntimeSubmission {
            api_version: request.api_version,
            context: request.context,
            control_fence: request.control_fence,
            body,
            coordination_receipt: None,
        }))
    }

    /// Construct a read envelope using a host-generated query correlation context.
    /// A `Watch` adapter combines retained catch-up with buffered notifications.
    ///
    /// # Errors
    /// Rejects cross-workspace/audience query identities or nested cursors/keys.
    pub fn coordination_query(
        &self,
        context: RequestContext,
        limit: NonZeroU32,
    ) -> Result<Option<Request<Query>>, SessionError> {
        self.check_request_context(&context)?;
        let body = match &self.action {
            SessionAction::Snapshot => Query::Snapshot,
            SessionAction::Watch { after } => {
                validation::cursor(after, &self.context)?;
                Query::CatchUp {
                    after: after.into(),
                    limit,
                }
            }
            SessionAction::RequestStatus(key) => {
                if key.workspace_id != self.context.workspace_id
                    || key.contributor_id != self.context.contributor_id
                {
                    return Err(validation::invalid(
                        "Status lookup has a different original request scope",
                    ));
                }
                Query::RequestStatus(key.into())
            }
            SessionAction::InputStatus(input) => {
                if input.request.workspace_id != self.context.workspace_id {
                    return Err(validation::invalid(
                        "Input lookup has a different workspace",
                    ));
                }
                Query::InputStatus(input.into())
            }
            SessionAction::Mutate { .. } => return Ok(None),
        };
        Ok(Some(Request {
            api_version: ApiVersion::V1,
            context,
            control_fence: None,
            body,
        }))
    }

    /// Register the runner allocated by the host runtime using f20's absent check.
    /// The host must persist its explicit logical item binding and finish this
    /// registration before resolving the original effect as `SessionResult::Created`.
    ///
    /// # Errors
    /// Rejects a non-create action or runtime metadata differing from the create intent.
    pub fn registration_request(&self, session: Session) -> Result<Request<Command>, SessionError> {
        let SessionAction::Mutate {
            request,
            mutation: SessionMutation::Create(draft),
        } = &self.action
        else {
            return Err(validation::invalid(
                "Directory registration requires a runtime create operation",
            ));
        };
        let context = RequestContext::from(request);
        self.check_request_context(&context)?;
        if session.owner != context.contributor.contributor_id
            || session.title != draft.title
            || session.runtime.host_id.0 != draft.host_id
            || session.kind != SessionKind::Runner
            || session.parent.as_ref().map(|id| &id.0) != draft.parent.as_ref()
        {
            return Err(validation::invalid(
                "Allocated runner does not match its create intent",
            ));
        }
        validation::nonempty(&session.id.0)?;
        validation::nonempty(&session.runtime.runtime_id.0)?;
        Ok(Request {
            api_version: ApiVersion::V1,
            context,
            control_fence: None,
            body: Command::PutSession(Change {
                expected: WriteCondition::Absent,
                value: session,
            }),
        })
    }

    fn check_request_context(&self, context: &RequestContext) -> Result<(), SessionError> {
        validation::context(&self.context)?;
        validation::contributor(&(&context.contributor).into())?;
        validation::nonempty(&context.request_id.0)?;
        if context.workspace_id.0 != self.context.workspace_id
            || context.contributor.contributor_id.0 != self.context.contributor_id
        {
            return Err(validation::invalid(
                "Request attribution does not match the authenticated session context",
            ));
        }
        Ok(())
    }
}
