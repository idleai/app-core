use crux_core::Request;

use super::{
    ConfigurationAction, ConfigurationContext, ConfigurationDocument, ConfigurationOperation,
    ConfigurationRecord, ConfigurationRequest, ConfigurationResult, ConfigurationSnapshot,
    ConfigurationValue, Event,
};
use crate::{Core, Effect, Event as RootEvent, workspace::WorkspaceMode};

mod recovery_cases;
mod root_cases;
mod state_cases;
mod wire_cases;

fn context(mode: WorkspaceMode) -> ConfigurationContext {
    ConfigurationContext {
        provider: "configuration".into(),
        workspace_id: "workspace".into(),
        contributor_id: "alice".into(),
        chain: "chain".into(),
        mode,
    }
}

fn record(revision: u64, json: &str) -> ConfigurationRecord {
    ConfigurationRecord {
        revision,
        value: ConfigurationValue {
            schema_version: 1,
            json: json.into(),
        },
    }
}

fn snapshot(
    operation: &ConfigurationOperation,
    record: Option<ConfigurationRecord>,
) -> ConfigurationSnapshot {
    ConfigurationSnapshot {
        context: operation.context.clone(),
        document: operation.document,
        record,
        can_edit: true,
    }
}

fn send(core: &Core, event: Event) -> Vec<Effect> {
    core.process_event(RootEvent::Configuration(event))
}

fn requests(effects: Vec<Effect>) -> Vec<Request<ConfigurationOperation>> {
    effects
        .into_iter()
        .filter_map(|effect| {
            if let Effect::Configuration(request) = effect {
                Some(*request)
            } else {
                None
            }
        })
        .collect()
}

fn request(
    effects: Vec<Effect>,
    document: ConfigurationDocument,
) -> Request<ConfigurationOperation> {
    requests(effects)
        .into_iter()
        .find(|request| request.operation.document == document)
        .expect("configuration request")
}

fn resolve_load(
    core: &Core,
    request: &mut Request<ConfigurationOperation>,
    record: Option<ConfigurationRecord>,
) -> Vec<Effect> {
    let result = ConfigurationResult::Loaded(snapshot(&request.operation, record));
    core.resolve(request, Ok(result))
        .expect("configuration load response")
}

fn ready(mode: WorkspaceMode) -> Core {
    let core = Core::new();
    for mut request in requests(send(&core, Event::Connect(context(mode)))) {
        let _effects = resolve_load(&core, &mut request, Some(record(3, "{}")));
    }
    core
}

fn edit(core: &Core, document: ConfigurationDocument, json: &str) {
    let _effects = send(
        core,
        Event::Edit {
            document,
            json: json.into(),
        },
    );
}

fn identity(id: &str) -> ConfigurationRequest {
    ConfigurationRequest {
        request_id: id.into(),
        expires_at_ms: 1000,
    }
}

fn save(core: &Core, document: ConfigurationDocument, id: &str) -> Request<ConfigurationOperation> {
    request(
        send(
            core,
            Event::Save {
                document,
                request: identity(id),
            },
        ),
        document,
    )
}

fn commit(
    core: &Core,
    request: &mut Request<ConfigurationOperation>,
    revision: u64,
) -> Vec<Effect> {
    let save = match &request.operation.action {
        ConfigurationAction::Save(save) => Some(save),
        ConfigurationAction::Load => None,
    }
    .expect("save operation required");
    let result = ConfigurationResult::Saved {
        request: save.request.clone(),
        snapshot: snapshot(
            &request.operation,
            Some(ConfigurationRecord {
                revision,
                value: save.value.clone(),
            }),
        ),
    };
    core.resolve(request, Ok(result))
        .expect("configuration commit response")
}
