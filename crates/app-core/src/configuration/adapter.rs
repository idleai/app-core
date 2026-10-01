//! Conversion to the shared coordination envelope, without a transport dependency.

use idle_protocol::v1::{
    ApiVersion, Change, WriteCondition,
    api::Request,
    identity::{ContributorIdentity, RequestContext, RequestId, Revision, Timestamp, WorkspaceId},
};
use serde::{Deserialize, Serialize};

use super::{
    ConfigurationAction, ConfigurationDocument, ConfigurationError, ConfigurationErrorKind,
    ConfigurationOperation, ConfigurationValue, validation,
};

/// Provider-neutral body for a conditional configuration write.
/// Providers own authorization, atomic persistence and durable notifications.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ConfigurationWrite {
    /// Independent settings or rules revision scope.
    pub document: ConfigurationDocument,
    /// No blind overwrite: an explicit revision or create-if-absent precondition.
    pub change: Change<ConfigurationValue>,
}

impl ConfigurationOperation {
    /// Build the existing v1 authenticated request envelope for either provider.
    /// Hosts persist this unchanged envelope before sending. The identity must
    /// come from the authenticated connection, never from the editor text.
    ///
    /// # Errors
    /// Rejects reads, invalid writes and a contributor from another connection.
    pub fn write_request(
        &self,
        contributor: ContributorIdentity,
    ) -> Result<Request<ConfigurationWrite>, ConfigurationError> {
        let ConfigurationAction::Save(save) = &self.action else {
            return Err(validation::error(
                ConfigurationErrorKind::InvalidInput,
                "A read has no write envelope",
            ));
        };
        validation::context(&self.context)?;
        validation::value(&save.value)?;
        if contributor.contributor_id.0 != self.context.contributor_id {
            return Err(validation::error(
                ConfigurationErrorKind::Unauthenticated,
                "Authenticated contributor does not match the configuration context",
            ));
        }
        if save.request.request_id.trim().is_empty()
            || save.request.expires_at_ms == 0
            || save.expected_revision == Some(0)
        {
            return Err(validation::error(
                ConfigurationErrorKind::InvalidInput,
                "Invalid configuration save identity or precondition",
            ));
        }
        Ok(Request {
            api_version: ApiVersion::V1,
            context: RequestContext {
                workspace_id: WorkspaceId(self.context.workspace_id.clone()),
                request_id: RequestId(save.request.request_id.clone()),
                contributor,
                expires_at: Timestamp(save.request.expires_at_ms),
            },
            control_fence: None,
            body: ConfigurationWrite {
                document: self.document,
                change: Change {
                    expected: save
                        .expected_revision
                        .map_or(WriteCondition::Absent, |revision| {
                            WriteCondition::Revision(Revision(revision))
                        }),
                    value: save.value.clone(),
                },
            },
        })
    }
}
