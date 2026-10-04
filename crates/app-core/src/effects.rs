//! Typed operations executed by hosts and their serializable bridge counterparts.

use crux_core::{
    EffectFFI, Request,
    bridge::{FfiFormat, ResolveSerialized},
    capability::Operation,
    render::RenderOperation,
};
use serde::{Deserialize, Serialize};

use crate::configuration::{ConfigurationOperation, ConfigurationOutput};
use crate::history::{Query, QueryOutput};
use crate::module::EffectError;
use crate::projections::{ProjectionOutput, ProjectionQuery};
use crate::repository::{RepositoryOutput, RepositoryQuery};
use crate::resources::{ResourceOperation, ResourceOutput};
use crate::sessions::{SessionOperation, SessionOutput};
use crate::subscriptions::{SubscriptionOperation, SubscriptionOutput};
use crate::workspace::{WorkspaceOperation, WorkspaceOutput};

/// Information supplied by the embedding client, not inferred by the core.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[serde(deny_unknown_fields)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct HostInfo {
    /// Human-readable client name, for example `Idle Web`.
    pub name: String,
    /// Host application version, independent of the app-core protocol version.
    pub version: String,
}

/// Result of the host information operation.
pub type HostInfoResult = Result<HostInfo, EffectError>;

/// Named wire result with generated serializers in shells.
/// Facet's generator needs an explicit enum; its layout matches [`HostInfoResult`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum HostInfoResponse {
    /// The host supplied its information.
    Ok(HostInfo),
    /// A presentable host failure.
    Err(EffectError),
}

impl From<HostInfoResult> for HostInfoResponse {
    fn from(result: HostInfoResult) -> Self {
        match result {
            Ok(info) => Self::Ok(info),
            Err(error) => Self::Err(error),
        }
    }
}

/// Ask the embedding host for its client information.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HostInfoOperation;

impl Operation for HostInfoOperation {
    type Output = HostInfoResult;
}

/// Requests emitted by reducers; Rust hosts resolve the typed request directly.
#[derive(Debug)]
pub enum Effect {
    /// Read the current view and schedule presentation; no response is expected.
    Render(Request<RenderOperation>),
    /// Execute the host information operation and return its typed result.
    HostInfo(Request<HostInfoOperation>),
    /// Execute a chain-scoped history query or platform history action.
    History(Box<Request<Query>>),
    /// Execute authorized workspace discovery, membership or presence reads.
    Workspace(Box<Request<WorkspaceOperation>>),
    /// Execute authorized joins, buffered change watches, release or retry timers.
    Subscription(Box<Request<SubscriptionOperation>>),
    /// Execute session creation, sharing, attributed input or retained recovery.
    Session(Box<Request<SessionOperation>>),
    /// Read a replacement of all five projection destinations.
    Projection(Box<Request<ProjectionQuery>>),
    /// Discover resources or execute/reconcile an authorized runtime action.
    Resource(Box<Request<ResourceOperation>>),
    /// Load or conditionally save settings/rules through either provider.
    Configuration(Box<Request<ConfigurationOperation>>),
    /// Read Git/GitHub details and recorded sessions for the selected repository.
    Repository(Box<Request<RepositoryQuery>>),
}

impl crux_core::Effect for Effect {}

impl From<Request<RenderOperation>> for Effect {
    fn from(request: Request<RenderOperation>) -> Self {
        Self::Render(request)
    }
}

impl From<Request<HostInfoOperation>> for Effect {
    fn from(request: Request<HostInfoOperation>) -> Self {
        Self::HostInfo(request)
    }
}

impl From<Request<Query>> for Effect {
    fn from(request: Request<Query>) -> Self {
        Self::History(Box::new(request))
    }
}

impl From<Request<WorkspaceOperation>> for Effect {
    fn from(request: Request<WorkspaceOperation>) -> Self {
        Self::Workspace(Box::new(request))
    }
}

impl From<Request<SubscriptionOperation>> for Effect {
    fn from(request: Request<SubscriptionOperation>) -> Self {
        Self::Subscription(Box::new(request))
    }
}

impl From<Request<SessionOperation>> for Effect {
    fn from(request: Request<SessionOperation>) -> Self {
        Self::Session(Box::new(request))
    }
}

impl From<Request<ProjectionQuery>> for Effect {
    fn from(request: Request<ProjectionQuery>) -> Self {
        Self::Projection(Box::new(request))
    }
}

impl From<Request<ResourceOperation>> for Effect {
    fn from(request: Request<ResourceOperation>) -> Self {
        Self::Resource(Box::new(request))
    }
}

impl From<Request<ConfigurationOperation>> for Effect {
    fn from(request: Request<ConfigurationOperation>) -> Self {
        Self::Configuration(Box::new(request))
    }
}

impl From<Request<RepositoryQuery>> for Effect {
    fn from(request: Request<RepositoryQuery>) -> Self {
        Self::Repository(Box::new(request))
    }
}

/// Serializable operations, without Rust continuation handles.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum EffectFfi {
    /// A notification; call `view`, and do not return a response for this ID.
    Render,
    /// Return a [`HostInfoResult`] for this request ID.
    HostInfo,
    /// Return a history query result for this request ID.
    History(Box<Query>),
    /// Return a workspace result for this request ID.
    Workspace(Box<WorkspaceOperation>),
    /// Return a subscription response for this request ID.
    Subscription(Box<SubscriptionOperation>),
    /// Return a session response for this request ID.
    Session(Box<SessionOperation>),
    /// Return a projection response for this request ID.
    Projection(Box<ProjectionQuery>),
    /// Return a resource response for this request ID.
    Resource(Box<ResourceOperation>),
    /// Return a configuration response for this request ID.
    Configuration(Box<ConfigurationOperation>),
    /// Return repository details and recorded-session source records.
    Repository(Box<RepositoryQuery>),
}

impl EffectFfi {
    pub(crate) const fn expects_response(&self) -> bool {
        match self {
            Self::Render => false,
            Self::HostInfo
            | Self::History(_)
            | Self::Workspace(_)
            | Self::Subscription(_)
            | Self::Session(_)
            | Self::Projection(_)
            | Self::Resource(_)
            | Self::Configuration(_)
            | Self::Repository(_) => true,
        }
    }

    pub(crate) fn validate_response(&self, bytes: &[u8]) -> Result<(), bincode::Error> {
        match self {
            Self::Render => Ok(()),
            Self::HostInfo => {
                let _result: HostInfoResult = crate::shell::ShellFormat::deserialize(bytes)?;
                Ok(())
            }
            Self::History(_) => {
                let _result: QueryOutput = crate::shell::ShellFormat::deserialize(bytes)?;
                Ok(())
            }
            Self::Workspace(_) => {
                let _result: WorkspaceOutput = crate::shell::ShellFormat::deserialize(bytes)?;
                Ok(())
            }
            Self::Subscription(_) => {
                let _result: SubscriptionOutput = crate::shell::ShellFormat::deserialize(bytes)?;
                Ok(())
            }
            Self::Session(_) => {
                let _result: SessionOutput = crate::shell::ShellFormat::deserialize(bytes)?;
                Ok(())
            }
            Self::Resource(_) => {
                let _result: ResourceOutput = crate::shell::ShellFormat::deserialize(bytes)?;
                Ok(())
            }
            Self::Configuration(_) => {
                let _result: ConfigurationOutput = crate::shell::ShellFormat::deserialize(bytes)?;
                Ok(())
            }
            Self::Projection(_) => {
                let _result: ProjectionOutput = crate::shell::ShellFormat::deserialize(bytes)?;
                Ok(())
            }
            Self::Repository(_) => {
                let _result: RepositoryOutput = crate::shell::ShellFormat::deserialize(bytes)?;
                Ok(())
            }
        }
    }
}

impl EffectFFI for Effect {
    type Ffi = EffectFfi;

    fn serialize<F: FfiFormat>(self) -> (Self::Ffi, ResolveSerialized<F>) {
        match self {
            Self::Render(request) => request.serialize(|_| EffectFfi::Render),
            Self::HostInfo(request) => request.serialize(|_| EffectFfi::HostInfo),
            Self::History(request) => {
                request.serialize(|query| EffectFfi::History(Box::new(query)))
            }
            Self::Workspace(request) => {
                request.serialize(|operation| EffectFfi::Workspace(Box::new(operation)))
            }
            Self::Subscription(request) => {
                request.serialize(|operation| EffectFfi::Subscription(Box::new(operation)))
            }
            Self::Session(request) => {
                request.serialize(|operation| EffectFfi::Session(Box::new(operation)))
            }
            Self::Resource(request) => {
                request.serialize(|operation| EffectFfi::Resource(Box::new(operation)))
            }
            Self::Configuration(request) => {
                request.serialize(|operation| EffectFfi::Configuration(Box::new(operation)))
            }
            Self::Projection(request) => {
                request.serialize(|operation| EffectFfi::Projection(Box::new(operation)))
            }
            Self::Repository(request) => {
                request.serialize(|operation| EffectFfi::Repository(Box::new(operation)))
            }
        }
    }
}
