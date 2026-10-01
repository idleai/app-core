//! Lossless adapters between portable shell payloads and f20 coordination types.

mod inputs;
mod metadata;
mod requests;

use idle_protocol::v1::{
    api::{ApiError, ErrorCode, RetryAdvice},
    identity::{ContributorIdentity, ExternalIdentity, RequestContext, RequestKey, Timestamp},
};

use super::{
    SessionCapabilities, SessionContext, SessionContributor, SessionError, SessionErrorCode,
    SessionMutationId, SessionRequest, SessionRequestKey, SessionRetryAdvice,
};

/// Authenticated host context used to project f20 recovery into session payloads.
/// Capabilities remain unavailable unless a production or explicitly scripted
/// adapter is connected; directory metadata alone cannot enable runtime actions.
#[derive(Clone, Debug)]
pub struct SessionAdapterContext {
    /// Provider/audience/workspace scope for this read.
    pub context: SessionContext,
    /// Identity verified by the authenticated host connection.
    pub contributor: ContributorIdentity,
    /// Capabilities supplied by the actual connected adapters.
    pub capabilities: SessionCapabilities,
    /// Provider-aligned current Unix time for expiry checks.
    pub now_ms: u64,
}

impl From<&ContributorIdentity> for SessionContributor {
    fn from(value: &ContributorIdentity) -> Self {
        Self {
            contributor_id: value.contributor_id.0.clone(),
            issuer: value.authenticated_as.issuer.clone(),
            subject: value.authenticated_as.subject.clone(),
        }
    }
}

impl From<&SessionContributor> for ContributorIdentity {
    fn from(value: &SessionContributor) -> Self {
        Self {
            contributor_id: value.contributor_id.as_str().into(),
            authenticated_as: ExternalIdentity {
                issuer: value.issuer.clone(),
                subject: value.subject.clone(),
            },
        }
    }
}

impl From<&RequestKey> for SessionRequestKey {
    fn from(value: &RequestKey) -> Self {
        Self {
            workspace_id: value.workspace_id.0.clone(),
            contributor_id: value.contributor_id.0.clone(),
            request_id: value.request_id.0.clone(),
        }
    }
}

impl From<&SessionRequestKey> for RequestKey {
    fn from(value: &SessionRequestKey) -> Self {
        Self {
            workspace_id: value.workspace_id.as_str().into(),
            contributor_id: value.contributor_id.as_str().into(),
            request_id: value.request_id.as_str().into(),
        }
    }
}

impl From<&RequestContext> for SessionRequest {
    fn from(value: &RequestContext) -> Self {
        Self {
            workspace_id: value.workspace_id.0.clone(),
            contributor: (&value.contributor).into(),
            mutation: SessionMutationId {
                request_id: value.request_id.0.clone(),
                expires_at_ms: value.expires_at.0,
            },
        }
    }
}

impl From<&SessionRequest> for RequestContext {
    fn from(value: &SessionRequest) -> Self {
        Self {
            workspace_id: value.workspace_id.as_str().into(),
            contributor: (&value.contributor).into(),
            request_id: value.mutation.request_id.as_str().into(),
            expires_at: Timestamp(value.mutation.expires_at_ms),
        }
    }
}

impl From<ApiError> for SessionError {
    fn from(value: ApiError) -> Self {
        let code = match value.code {
            ErrorCode::InvalidRequest => SessionErrorCode::InvalidRequest,
            ErrorCode::UnsupportedVersion => SessionErrorCode::UnsupportedVersion,
            ErrorCode::UnsupportedOperation => SessionErrorCode::UnsupportedOperation,
            ErrorCode::Unauthenticated => SessionErrorCode::Unauthenticated,
            ErrorCode::Forbidden => SessionErrorCode::Forbidden,
            ErrorCode::NotFound => SessionErrorCode::NotFound,
            ErrorCode::Conflict => SessionErrorCode::Conflict,
            ErrorCode::StaleRevision => SessionErrorCode::StaleRevision,
            ErrorCode::StaleControl => SessionErrorCode::StaleControl,
            ErrorCode::IdempotencyConflict => SessionErrorCode::IdempotencyConflict,
            ErrorCode::RequestExpired => SessionErrorCode::RequestExpired,
            ErrorCode::CursorScopeMismatch => SessionErrorCode::CursorScopeMismatch,
            ErrorCode::Unavailable => SessionErrorCode::Unavailable,
            ErrorCode::RateLimited => SessionErrorCode::RateLimited,
        };
        let retry = match value.retry {
            RetryAdvice::Never => SessionRetryAdvice::Never,
            RetryAdvice::SameRequest { not_before } => SessionRetryAdvice::SameRequest {
                not_before_ms: not_before.map(|time| time.0),
            },
            RetryAdvice::QueryStatus => SessionRetryAdvice::QueryStatus,
        };
        Self {
            code,
            message: value.message,
            retry,
        }
    }
}
