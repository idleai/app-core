//! Versioned binary boundary for foreign-language hosts.

use std::{collections::BTreeMap, fmt, sync::Mutex};

use bincode::Options;
use crux_core::bridge::{BincodeFfiFormat, Bridge, BridgeError, EffectId, FfiFormat};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{Core, IdleApp, effects::EffectFfi};

/// Version of the app-core shell protocol (independent of coordination APIs).
pub const PROTOCOL_VERSION: u32 = 8;

/// Crux's fixed-width, little-endian bincode format, rejecting trailing input.
#[derive(Debug)]
pub struct ShellFormat;

impl FfiFormat for ShellFormat {
    type Error = bincode::Error;

    fn serialize<T: Serialize>(buffer: &mut Vec<u8>, value: &T) -> Result<(), Self::Error> {
        BincodeFfiFormat::serialize(buffer, value)
    }

    fn deserialize<'de, T: Deserialize<'de>>(bytes: &'de [u8]) -> Result<T, Self::Error> {
        bincode::DefaultOptions::new()
            .with_fixint_encoding()
            .reject_trailing_bytes()
            .deserialize(bytes)
    }
}

/// Effect batch returned to hosts, with generated deserializers.
/// Its single sequence field matches Crux's serialized request vector.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct EffectBatch {
    /// Effects to execute, including render notifications.
    pub requests: Vec<EffectRequest>,
}

/// Serializable effect request; IDs are local to one live shell instance.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct EffectRequest {
    /// Echo this ID when resolving the operation; never persist or share it.
    pub id: u32,
    /// Operation for the host adapter to execute.
    pub effect: EffectFfi,
}

/// Errors at the shell boundary, distinct from an operation's domain error.
#[derive(Debug, Error)]
pub enum ShellError {
    /// Bytes did not match an operation result; the request remains pending.
    #[error("invalid host response: {0}")]
    InvalidResponse(bincode::Error),
    /// The ID is unknown, already completed, or belongs to a notification.
    #[error("no pending operation for request {0}")]
    UnknownRequest(u32),
    /// The core produced an invalid effect batch.
    #[error("invalid core effect batch: {0}")]
    InvalidRequests(bincode::Error),
    /// Crux rejected an event, response ID or serialization operation.
    #[error(transparent)]
    Bridge(#[from] BridgeError<ShellFormat>),
    /// A prior panic poisoned this instance; create a new shell.
    #[error("the shell lock was poisoned")]
    Poisoned,
}

/// One independent application's serialized interface.
///
/// Calls on an instance are serialized, including response resolution, so native
/// callers cannot interleave a model update with another call's effect draining.
/// The lock is never held while the host executes an effect.
pub struct Shell {
    state: Mutex<ShellState>,
}

struct ShellState {
    bridge: Bridge<IdleApp, ShellFormat>,
    // Crux owns continuations and IDs. This table only remembers each pending
    // operation's wire type so validation happens before a callback is consumed.
    pending: BTreeMap<u32, EffectFfi>,
}

impl ShellState {
    fn register(&mut self, bytes: &[u8]) -> Result<(), ShellError> {
        let batch: EffectBatch =
            ShellFormat::deserialize(bytes).map_err(ShellError::InvalidRequests)?;
        self.pending.extend(
            batch
                .requests
                .into_iter()
                .filter(|request| request.effect.expects_response())
                .map(|request| (request.id, request.effect)),
        );
        Ok(())
    }
}

impl fmt::Debug for Shell {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Shell").finish_non_exhaustive()
    }
}

impl Default for Shell {
    fn default() -> Self {
        Self::new()
    }
}

impl Shell {
    /// Create an isolated client model and effect registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Mutex::new(ShellState {
                bridge: Bridge::new(Core::new()),
                pending: BTreeMap::new(),
            }),
        }
    }

    /// Process a bincode client event and return an encoded [`EffectBatch`].
    ///
    /// # Errors
    /// Returns an error for malformed or internal-only events, or a poisoned core.
    pub fn process_event(&self, event: &[u8]) -> Result<Vec<u8>, ShellError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_poisoned| ShellError::Poisoned)?;
        let mut output = Vec::new();
        state.bridge.update(event, &mut output)?;
        state.register(&output)?;
        Ok(output)
    }

    /// Resolve a bincode result and return an encoded follow-up [`EffectBatch`].
    ///
    /// # Errors
    /// Rejects malformed results, unknown, completed or notification-only IDs,
    /// and poisoned instances. Malformed results can be corrected and retried.
    pub fn handle_response(&self, id: u32, response: &[u8]) -> Result<Vec<u8>, ShellError> {
        // Crux consumes a one-shot continuation before deserializing its output.
        // Validate the output first so malformed input cannot strand a pending load.
        let mut state = self
            .state
            .lock()
            .map_err(|_poisoned| ShellError::Poisoned)?;
        let operation = state
            .pending
            .get(&id)
            .ok_or(ShellError::UnknownRequest(id))?;
        operation
            .validate_response(response)
            .map_err(ShellError::InvalidResponse)?;
        let mut output = Vec::new();
        state.bridge.resolve(EffectId(id), response, &mut output)?;
        let _completed = state.pending.remove(&id);
        state.register(&output)?;
        Ok(output)
    }

    /// Read the typed view model as bincode bytes.
    ///
    /// # Errors
    /// Returns an error if serialization fails or the instance was poisoned.
    pub fn view(&self) -> Result<Vec<u8>, ShellError> {
        let state = self
            .state
            .lock()
            .map_err(|_poisoned| ShellError::Poisoned)?;
        let mut output = Vec::new();
        state.bridge.view(&mut output)?;
        Ok(output)
    }
}
