//! Exercise Crux continuations against real engine queries and stored records.

#[cfg(not(target_arch = "wasm32"))]
mod engine_cases;
mod state_cases;
mod wire_cases;

use crux_core::Request;
use editchain_core::{
    Op, OpId, Payload,
    activity::{ContentUpdate, ItemId, Kind, Message, MessageKind, Operation, Stage, UpdateMode},
};

use super::{Event, HistoryPage, Observation, Query, QueryResult, RecordRef};
use crate::{Core, Effect, Event as RootEvent};

fn id(number: u8) -> OpId {
    OpId::from_bytes([number; 32])
}

fn message(number: u8, previous: Option<u8>, bytes: &[u8]) -> Op {
    let mut activity = Operation::new(
        id(number),
        ItemId(id(200)),
        ItemId(id(201)),
        Kind::Message(Message {
            category: MessageKind::Text,
            stage: Stage::Updated,
            audience: Vec::new(),
            blocks: vec![ContentUpdate {
                block: ItemId(id(202)),
                position: Some(0),
                mode: if previous.is_some() {
                    UpdateMode::Append
                } else {
                    UpdateMode::Replace
                },
                previous: previous.map(id),
                media_type: Payload::Inline(b"text/plain".to_vec()),
                content: Payload::Inline(bytes.to_vec()),
            }],
            coverage: None,
            outcome: None,
        }),
    );
    activity.author = Some(ItemId(id(203)));
    activity.session = Some(ItemId(id(204)));
    activity.into_op().expect("valid message fixture")
}

fn observation(op: &Op) -> Observation {
    Observation {
        record: RecordRef {
            operation: op.id.to_string(),
            hash: id(250).to_string(),
        },
        operation_json: serde_json::to_vec(op).expect("shared operation JSON codec"),
    }
}

fn requests(effects: Vec<Effect>) -> Vec<Request<Query>> {
    effects
        .into_iter()
        .filter_map(|effect| match effect {
            Effect::History(request) => Some(*request),
            Effect::Render(_) | Effect::HostInfo(_) | Effect::Workspace(_) => None,
        })
        .collect()
}

fn send(core: &Core, event: Event) -> Vec<Request<Query>> {
    requests(core.process_event(RootEvent::History(event)))
}

fn first(requests: Vec<Request<Query>>) -> Request<Query> {
    let mut iter = requests.into_iter();
    let request = iter.next().expect("one history query");
    assert!(iter.next().is_none(), "only one history query expected");
    request
}

fn page(core: &Core, request: &mut Request<Query>, ops: &[Op], next: Option<OpId>) {
    let follow_up = core
        .resolve(
            request,
            Ok(QueryResult::History(HistoryPage {
                observations: ops.iter().map(observation).collect(),
                next_after: next.map(|id| id.to_string()),
                scanned: u32::try_from(ops.len()).expect("bounded page"),
            })),
        )
        .expect("resolve page");
    assert!(
        requests(follow_up).is_empty(),
        "paging is controlled by client events"
    );
}
