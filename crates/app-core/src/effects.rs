//! Typed operations executed by hosts and their serializable bridge counterparts.

use crux_core::{
    EffectFFI, Request,
    bridge::{FfiFormat, ResolveSerialized},
    capability::Operation,
    render::RenderOperation,
};
use serde::{Deserialize, Serialize};

use crate::module::EffectError;

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

/// Serializable operations, without Rust continuation handles.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum EffectFfi {
    /// A notification; call `view`, and do not return a response for this ID.
    Render,
    /// Return a [`HostInfoResult`] for this request ID.
    HostInfo,
}

impl EffectFfi {
    pub(crate) const fn expects_response(self) -> bool {
        match self {
            Self::Render => false,
            Self::HostInfo => true,
        }
    }

    pub(crate) fn validate_response(self, bytes: &[u8]) -> Result<(), bincode::Error> {
        match self {
            Self::Render => Ok(()),
            Self::HostInfo => {
                let _result: HostInfoResult = crate::shell::ShellFormat::deserialize(bytes)?;
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
        }
    }
}
