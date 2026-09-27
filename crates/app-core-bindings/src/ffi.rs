//! `BoltFFI` native interfaces over the app-core binary shell.

use app_core::Shell;
use thiserror::Error;

/// An invalid bridge call, reported as an exception to the host.
#[boltffi::error]
#[derive(Debug, Error)]
pub enum BindingError {
    /// The shell rejected the input or could not process it.
    #[error("{details}")]
    Shell {
        /// A diagnostic for the adapter, distinct from user-facing effect failures.
        /// Named to avoid the inherited `Throwable.message` in Kotlin exceptions.
        details: String,
    },
}

impl From<app_core::ShellError> for BindingError {
    fn from(error: app_core::ShellError) -> Self {
        Self::Shell {
            details: error.to_string(),
        }
    }
}

/// One client instance. Each instance owns its own model and pending effects.
#[derive(Debug, Default)]
pub struct AppCore {
    shell: Shell,
}

#[boltffi::export]
impl AppCore {
    /// Create a client. Release it using the generated binding's lifetime API.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the binary shell protocol version supported by this instance.
    #[must_use]
    pub fn protocol_version(&self) -> u32 {
        app_core::shell::PROTOCOL_VERSION
    }

    /// Accept a bincode client event and return an encoded effect batch.
    ///
    /// # Errors
    /// Throws when the event is invalid or the instance cannot process it.
    pub fn process_event(&self, event: &[u8]) -> Result<Vec<u8>, BindingError> {
        Ok(self.shell.process_event(event)?)
    }

    /// Return a bincode effect result with the ID supplied by this instance.
    ///
    /// # Errors
    /// Throws for invalid results, unknown or completed IDs, or a failed instance.
    pub fn handle_response(&self, id: u32, response: &[u8]) -> Result<Vec<u8>, BindingError> {
        Ok(self.shell.handle_response(id, response)?)
    }

    /// Read a bincode snapshot of the shared view model.
    ///
    /// # Errors
    /// Throws when the instance cannot serialize its current view.
    pub fn view(&self) -> Result<Vec<u8>, BindingError> {
        Ok(self.shell.view()?)
    }
}
