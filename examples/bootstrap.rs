//! Exercise a client event, its render effect and the resulting view.

use std::io::{self, Write as _};

use app_core::{Core, Effect, Event};
// These dependencies are used by the library target linked by this example.
use crux_core as _;
use serde as _;

fn main() -> io::Result<()> {
    let core = Core::new();
    if core.view().initialized {
        return Err(io::Error::other("a new client must start uninitialized"));
    }
    let effects = core.process_event(Event::Start);
    if !matches!(effects.as_slice(), [Effect::Render(_)]) {
        return Err(io::Error::other(
            "initializing the client must request one render",
        ));
    }
    if !core.view().initialized {
        return Err(io::Error::other("the start event must initialize the view"));
    }
    writeln!(
        io::stdout(),
        "app-core: bootstrap event, render effect, and view succeeded"
    )
}
