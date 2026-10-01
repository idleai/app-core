use crux_core::Request;

use super::{
    Event, FreshnessStatus, ProjectionAvailability, ProjectionFreshness, ProjectionInput,
    ProjectionKind, ProjectionQuery, ProjectionReference, ProjectionRow, ProjectionSnapshot,
};
use crate::{Core, Effect, Event as RootEvent, subscriptions::Context};

#[cfg(not(target_arch = "wasm32"))]
mod engine_cases;
mod root_cases;
mod state_cases;
mod wire_cases;

fn context() -> Context {
    Context {
        provider: "standalone".into(),
        workspace: "workspace".into(),
        contributor: "alice".into(),
        chain: "chain".into(),
    }
}

fn reference() -> ProjectionReference {
    ProjectionReference {
        observation: Some("10".repeat(32)),
        item: Some("a0".repeat(32)),
        record_hash: Some("b0".repeat(32)),
    }
}

fn snapshot() -> ProjectionSnapshot {
    ProjectionSnapshot {
        version: 1,
        workspace_id: "workspace".into(),
        chain: "chain".into(),
        inputs: ProjectionKind::ALL
            .into_iter()
            .map(|kind| ProjectionInput {
                kind,
                freshness: ProjectionFreshness {
                    status: FreshnessStatus::Current,
                    generated_at_ms: Some(1000),
                    checkpoint: Some("opaque/checkpoint".into()),
                },
                availability: ProjectionAvailability::Complete,
                total: Some(1),
                gaps: Vec::new(),
                rows: vec![ProjectionRow {
                    key: "stable-row".into(),
                    title: "Check exact text".into(),
                    summary: Some("Supplied details 🌍\n".into()),
                    status: Some("provider/active".into()),
                    labels: vec!["provider/label".into()],
                    sources: vec![reference()],
                    related: vec![ProjectionReference {
                        observation: None,
                        item: Some("c0".repeat(32)),
                        record_hash: None,
                    }],
                }],
            })
            .collect(),
    }
}

fn send(core: &Core, event: Event) -> Vec<Effect> {
    core.process_event(RootEvent::Projections(event))
}

fn request(effects: Vec<Effect>) -> Request<ProjectionQuery> {
    effects
        .into_iter()
        .find_map(|effect| match effect {
            Effect::Projection(request) => Some(*request),
            Effect::Render(_)
            | Effect::HostInfo(_)
            | Effect::History(_)
            | Effect::Workspace(_)
            | Effect::Subscription(_)
            | Effect::Session(_) => None,
        })
        .expect("projection request")
}

fn ready() -> Core {
    let core = Core::new();
    let mut load = request(send(&core, Event::Connect(context())));
    let _effects = core
        .resolve(&mut load, Ok(snapshot()))
        .expect("valid snapshot");
    core
}
