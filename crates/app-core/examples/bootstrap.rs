//! Run a client event, host-executed effect, returned result and typed view update.

use std::{
    error::Error,
    io::{self, Write as _},
};

use app_core::{Core, Effect, Event, effects::HostInfo, module::LoadState};
use bincode as _;
use crux_core as _;
use facet as _;
#[cfg(feature = "typegen")]
use facet_generate as _;
use serde as _;
use thiserror as _;

fn main() -> Result<(), Box<dyn Error>> {
    let core = Core::new();
    for effect in core.process_event(Event::Start) {
        match effect {
            Effect::Render(_) => {}
            Effect::History(_) => {
                return Err(io::Error::other("unexpected history operation").into());
            }
            Effect::Subscription(_) | Effect::Workspace(_) | Effect::Session(_) => {
                return Err(io::Error::other("unexpected workspace operation").into());
            }
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

use editchain_core as _;
#[cfg(not(target_arch = "wasm32"))]
use editchain_engine as _;
use idle_history as _;
use idle_protocol as _;
use serde_json as _;
use tempfile as _;
