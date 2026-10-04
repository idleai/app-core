use crux_core::capability::Operation;
use serde::{Deserialize, Serialize};

use super::RepositorySnapshot;
use crate::{module::EffectError, subscriptions::Context};

/// Authorized subscription plus its exact selected repository.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct RepositoryContext {
    /// Provider, audience, workspace and chain.
    pub connection: Context,
    /// Exact repository identity installed by the host.
    pub repository_id: String,
}

/// Read intents; account interaction occurs only after the explicit user action.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum RepositoryAction {
    /// Refresh Git, accessible GitHub data and recorded session items.
    Read,
    /// Refresh local records while allowing a recent GitHub read to be reused.
    Poll,
    /// Ask the host to connect a GitHub repository account, then read again.
    SignIn,
    /// Remember this view's selected recorded session under its host binding.
    Remember(Option<String>),
}

/// One correlated replacement request for the current repository context.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct RepositoryQuery {
    /// Exact authorized selection; hosts resolve paths independently.
    pub context: RepositoryContext,
    /// Requested read or explicit account connection.
    pub action: RepositoryAction,
}

/// Typed native or remote repository result.
pub type RepositoryOutput = Result<RepositoryResult, EffectError>;

/// Repository data and host-local selection acknowledgements remain distinct.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum RepositoryResult {
    /// Replacement data with this view's host-retained recorded-session selection.
    Snapshot {
        /// Bounded source results for the exact query scope.
        snapshot: Box<RepositorySnapshot>,
        /// Previously selected logical session, validated against the new snapshot.
        selected_session: Option<String>,
    },
    /// The host persisted this view's latest selection preference.
    Remembered,
}

/// Generated-shell result with the layout of [`RepositoryOutput`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum RepositoryResponse {
    /// Replacement repository data with per-source read reports.
    Ok(RepositoryResult),
    /// Presentable host failure.
    Err(EffectError),
}

impl Operation for RepositoryQuery {
    type Output = RepositoryOutput;
}

/// Repository read lifetime, independent of the source's declared completeness.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum RepositoryLoadState {
    /// No repository has been read.
    #[default]
    Idle,
    /// A replacement is pending; previous results are stale.
    Loading,
    /// The last replacement was admitted.
    Ready,
    /// Delivery continuity was lost; retained data is stale.
    Suspended,
    /// The read failed; retained data is stale.
    Failed(EffectError),
}

/// Shared repository and recorded-session presentation state.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet reflection adds no safety invariants"
)]
pub struct RepositoryViewModel {
    /// Current exact repository selection.
    pub context: Option<RepositoryContext>,
    /// Last admitted replacement; cleared on provider, audience or binding changes.
    pub snapshot: Option<RepositorySnapshot>,
    /// Full logical recorded-session identity, retained across refresh.
    pub selected_session: Option<String>,
    /// Correlated request state.
    pub load: RepositoryLoadState,
    /// Retained data requires a fresh replacement.
    pub needs_refresh: bool,
    /// Invalid user action, distinct from source read failures.
    pub action_error: Option<EffectError>,
}
