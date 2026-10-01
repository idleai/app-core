//! Host-executed reads and conditional, retry-safe saves.

use crux_core::capability::Operation;
use serde::{Deserialize, Serialize};

use super::{
    ConfigurationContext, ConfigurationDocument, ConfigurationSnapshot, ConfigurationValue,
};

/// Host-persisted retry identity, never a Crux continuation ID.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ConfigurationRequest {
    /// Unique across mutations for this contributor/workspace.
    pub request_id: String,
    /// Original first-receipt deadline in provider-clock Unix milliseconds.
    pub expires_at_ms: u64,
}

/// Immutable conditional write retained until a definite outcome is known.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ConfigurationSave {
    /// Retry identity and original deadline supplied by the host.
    pub request: ConfigurationRequest,
    /// Required current revision, or create only if never present.
    pub expected_revision: Option<u64>,
    /// Exact submitted value; editing while saving does not change it.
    pub value: ConfigurationValue,
}

/// Operations dispatched to the selected standalone or managed adapter.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ConfigurationAction {
    /// Read the latest authorized document and editing capability.
    Load,
    /// Atomically authorize, check revision, persist and retain the result.
    Save(ConfigurationSave),
}

/// A fully scoped configuration effect; persistence belongs to the provider.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ConfigurationOperation {
    /// Authenticated provider and workspace binding.
    pub context: ConfigurationContext,
    /// Independently versioned document.
    pub document: ConfigurationDocument,
    /// Read or conditional write.
    pub action: ConfigurationAction,
}

/// Presentable error classes; no sandbox or execution decision happens here.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ConfigurationErrorKind {
    /// Invalid client intent or intermediate JSON text.
    InvalidInput,
    /// Mismatched, malformed or regressing provider response.
    InvalidData,
    /// Provider requires authentication.
    Unauthenticated,
    /// Provider rejected current authorization.
    Forbidden,
    /// Revision or current configuration conflicts with the write.
    Conflict,
    /// Provider or transport cannot currently respond.
    Unavailable,
    /// Unsupported document format or operation.
    Unsupported,
}

/// Safe feedback without credentials or private diagnostics.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct ConfigurationError {
    /// Machine-readable class.
    pub kind: ConfigurationErrorKind,
    /// User-presentable explanation.
    pub message: String,
}

impl std::fmt::Display for ConfigurationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ConfigurationError {}

/// Provider-confirmed outcomes, distinct from transport-level failures.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ConfigurationResult {
    /// Authorized read, including a document that has never existed.
    Loaded(ConfigurationSnapshot),
    /// Persisted value and its authority-assigned revision, after durable commit.
    Saved {
        /// Unchanged original request identity and deadline.
        request: ConfigurationRequest,
        /// Committed document, including current editing capability.
        snapshot: ConfigurationSnapshot,
    },
    /// Definite, retained refusal: this exact request did not commit.
    Rejected {
        /// Original request, correlated by the adapter.
        request: ConfigurationRequest,
        /// Safe validation, concurrency or authorization feedback.
        error: ConfigurationError,
    },
}

/// A failed save transport is uncertain; only `Rejected` proves non-commit.
pub type ConfigurationOutput = Result<ConfigurationResult, ConfigurationError>;

/// Generated-shell result matching the Rust result's binary layout.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ConfigurationResponse {
    /// Provider-confirmed result.
    Ok(ConfigurationResult),
    /// Read error or uncertain save transport.
    Err(ConfigurationError),
}

impl Operation for ConfigurationOperation {
    type Output = ConfigurationOutput;
}
