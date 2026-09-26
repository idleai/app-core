//! Versioned JSON boundary shared by native and WASM bindings.

use std::{collections::BTreeMap, fmt, sync::Mutex};

use crux_core::bridge::{Bridge, BridgeError, EffectId, JsonFfiFormat};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{Core, IdleApp, effects::EffectFfi};

/// Version of the app-core shell protocol (independent of coordination APIs).
pub const PROTOCOL_VERSION: u32 = 1;

/// Serializable effect request; IDs are local to one live shell instance.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EffectRequest {
    /// Echo this ID when resolving the operation; never persist or share it.
    pub id: u32,
    /// Operation for the host adapter to execute.
    pub effect: EffectFfi,
}

/// Errors at the shell boundary, distinct from an operation's domain error.
#[derive(Debug, Error)]
pub enum ShellError {
    /// JSON did not match an operation result; the request remains pending.
    #[error("invalid host response: {0}")]
    InvalidResponse(serde_json::Error),
    /// The ID is unknown, already completed, or belongs to a notification.
    #[error("no pending operation for request {0}")]
    UnknownRequest(u32),
    /// The core produced an invalid effect batch.
    #[error("invalid core effect batch: {0}")]
    InvalidRequests(serde_json::Error),
    /// Crux rejected an event, response ID or serialization operation.
    #[error(transparent)]
    Bridge(#[from] BridgeError<JsonFfiFormat>),
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
    bridge: Bridge<IdleApp, JsonFfiFormat>,
    // Crux owns continuations and IDs. This table only remembers each pending
    // operation's wire type so validation happens before a callback is consumed.
    pending: BTreeMap<u32, EffectFfi>,
}

impl ShellState {
    fn register(&mut self, bytes: &[u8]) -> Result<(), ShellError> {
        let requests: Vec<EffectRequest> =
            serde_json::from_slice(bytes).map_err(ShellError::InvalidRequests)?;
        self.pending.extend(
            requests
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

    /// Process UTF-8 JSON for a client event and return JSON effect requests.
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

    /// Resolve one request with its JSON result and return any follow-up effects.
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

    /// Read the typed view model as UTF-8 JSON.
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

/// Schema entrypoint grouping the four wire payload types, not a wire envelope.
#[cfg(feature = "schema")]
#[derive(Debug, schemars::JsonSchema)]
pub struct ShellContract {
    /// Client actions accepted by `process_event`.
    pub event: crate::Event,
    /// Effect batches returned by event and response calls.
    pub requests: Vec<EffectRequest>,
    /// Output expected for a host information request.
    pub host_info_result: crate::effects::HostInfoResult,
    /// Presentation state returned by `view`.
    pub view: crate::ViewModel,
}
