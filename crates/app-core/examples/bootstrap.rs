//! Run a client event, host-executed effect, returned result and typed view update.

use std::{
    error::Error,
    io::{self, Write as _},
};

use app_core::{Core, Effect, Event, effects::HostInfo, module::LoadState};
use crux_core as _;
#[cfg(feature = "schema")]
use schemars as _;
use serde as _;
use serde_json as _;
use thiserror as _;

fn main() -> Result<(), Box<dyn Error>> {
    let core = Core::new();
    for effect in core.process_event(Event::Start) {
        match effect {
            Effect::Render(_) => {}
            Effect::HostInfo(mut request) => {
                // Host adapter work happens here, outside the reducer.
                let result = HostInfo {
                    name: "Rust example host".into(),
                    version: env!("CARGO_PKG_VERSION").into(),
                };
                let effects = core.resolve(&mut request, Ok(result))?;
                if !effects
                    .iter()
                    .all(|effect| matches!(effect, Effect::Render(_)))
                {
                    return Err(io::Error::other("unexpected follow-up operation").into());
                }
            }
        }
    }
    let view = core.view();
    if !view.initialized || !matches!(view.bootstrap, LoadState::Ready(_)) {
        return Err(io::Error::other("host result did not update the typed view").into());
    }
    writeln!(
        io::stdout(),
        "app-core: event -> host effect -> result -> typed view: {view:?}"
    )?;
    Ok(())
}
