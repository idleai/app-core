//! Generated native and JavaScript interfaces over the same app-core JSON shell.

use app_core::Shell;
use thiserror::Error;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

#[cfg(not(target_arch = "wasm32"))]
uniffi::setup_scaffolding!();

/// An invalid bridge call, reported as an exception to the host.
#[derive(Debug, Error)]
#[cfg_attr(not(target_arch = "wasm32"), derive(uniffi::Error))]
pub enum BindingError {
    /// The shell rejected the input or could not process it.
    #[error("{message}")]
    Shell {
        /// A diagnostic for the adapter, distinct from user-facing effect failures.
        message: String,
    },
}

impl From<app_core::ShellError> for BindingError {
    fn from(error: app_core::ShellError) -> Self {
        Self::Shell {
            message: error.to_string(),
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl From<BindingError> for wasm_bindgen::JsValue {
    fn from(error: BindingError) -> Self {
        Self::from_str(&error.to_string())
    }
}

/// One client instance. Each instance owns its own model and pending effects.
#[derive(Debug, Default)]
#[cfg_attr(not(target_arch = "wasm32"), derive(uniffi::Object))]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
pub struct AppCore {
    shell: Shell,
}

#[cfg_attr(not(target_arch = "wasm32"), uniffi::export)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl AppCore {
    /// Create a client. Release it using the generated binding's lifetime API.
    #[must_use]
    #[cfg_attr(not(target_arch = "wasm32"), uniffi::constructor)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the JSON shell protocol version supported by this instance.
    #[must_use]
    pub fn protocol_version(&self) -> u32 {
        app_core::shell::PROTOCOL_VERSION
    }

    /// Accept a UTF-8 JSON client event and return a JSON effect request array.
    ///
    /// # Errors
    /// Throws when the event is invalid or the instance cannot process it.
    pub fn process_event(&self, event: &[u8]) -> Result<Vec<u8>, BindingError> {
        Ok(self.shell.process_event(event)?)
    }

    /// Return a UTF-8 JSON effect result with the ID supplied by this instance.
    ///
    /// # Errors
    /// Throws for invalid results, unknown or completed IDs, or a failed instance.
    pub fn handle_response(&self, id: u32, response: &[u8]) -> Result<Vec<u8>, BindingError> {
        Ok(self.shell.handle_response(id, response)?)
    }

    /// Read a UTF-8 JSON snapshot of the shared view model.
    ///
    /// # Errors
    /// Throws when the instance cannot serialize its current view.
    pub fn view(&self) -> Result<Vec<u8>, BindingError> {
        Ok(self.shell.view()?)
    }
}
