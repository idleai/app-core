use crux_core::Request;

use super::{Event, ResourceOperation, ResourceSnapshot, scripted};
use crate::{Core, Effect, Event as RootEvent, workspace::WorkspaceMode};

mod adapter_cases;
mod mutation_cases;
mod root_cases;
mod state_cases;
mod wire_cases;

fn snapshot() -> ResourceSnapshot {
    scripted::demo_snapshot(WorkspaceMode::Standalone).expect("resource fixture")
}

fn send(core: &Core, event: Event) -> Vec<Effect> {
    core.process_event(RootEvent::Resources(event))
}

fn request(effects: Vec<Effect>) -> Request<ResourceOperation> {
    effects
        .into_iter()
        .find_map(|effect| {
            if let Effect::Resource(request) = effect {
                Some(*request)
            } else {
                None
            }
        })
        .expect("resource effect")
}

fn ready_with(snapshot: ResourceSnapshot) -> Core {
    let core = Core::new();
    let mut load = request(send(&core, Event::Connect(snapshot.context.clone())));
    let _effects = core
        .resolve(
            &mut load,
            Ok(super::ResourceResult::Snapshot(Box::new(snapshot))),
        )
        .expect("resource snapshot response");
    core
}

fn ready() -> Core {
    ready_with(snapshot())
}
