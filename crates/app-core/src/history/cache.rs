//! Full observations and immutable stream replay, without presentation geometry.

use std::collections::{BTreeMap, BTreeSet};

use editchain_core::{
    Op, OpId, Payload,
    activity::{self, Kind, Operation, StreamState},
};

use super::{
    ActivityKind, BlockState, BlockView, CacheStatus, ContentText, ContentValue, Detail, Endpoint,
    ItemView, Model, Observation, ObservationView, OperationDetailsState, Paging,
};
use crate::module::EffectError;

/// Soft record bound: selected and expanded logical items are retained.
const MAX_CACHED_OBSERVATIONS: usize = 2000;

#[derive(Debug, Default)]
pub(super) struct Cache {
    records: BTreeMap<String, Cached>,
    items: BTreeMap<String, BTreeSet<String>>,
    order: Vec<String>,
    stream: StreamState,
    evicted: u64,
}

#[derive(Debug)]
struct Cached {
    key: String,
    kind: ActivityKind,
    wire: Observation,
    op: Op,
    activity: Option<Operation>,
    problem: Option<String>,
    unavailable: bool,
}

pub(super) fn full_id(value: &str) -> Result<OpId, EffectError> {
    idle_history::query::full_id(value).map_err(Into::into)
}

pub(super) fn error(message: &str) -> EffectError {
    EffectError {
        message: message.to_owned(),
    }
}

use idle_history::query::{item_key, kind};

impl Cache {
    pub(super) fn identities(&self) -> Vec<String> {
        self.records.keys().cloned().collect()
    }

    pub(super) fn observation(&self, operation: &str) -> Option<Observation> {
        self.records
            .get(operation)
            .map(|record| record.wire.clone())
    }
    pub(super) fn insert(&mut self, wire: Observation) -> Result<String, EffectError> {
        let op = wire.operation()?;
        let key = item_key(&op);
        if let Some(existing) = self.records.get_mut(&wire.record.operation) {
            if existing.wire.record != wire.record || existing.op != op {
                existing.problem = Some("Conflicting representations of one observation".into());
                existing.unavailable = false;
                if let Some(activity) = Operation::view(&op) {
                    let _replay = self.stream.insert(activity);
                }
            } else if existing.unavailable {
                existing.problem = None;
                existing.unavailable = false;
            }
            return Ok(existing.key.clone());
        }
        let activity = Operation::view(&op);
        let problem = activity
            .as_ref()
            .and_then(|activity| self.stream.insert(activity.clone()).err())
            .map(|error| error.to_string());
        self.order.push(wire.record.operation.clone());
        let _inserted = self
            .items
            .entry(key.clone())
            .or_default()
            .insert(wire.record.operation.clone());
        let _previous = self.records.insert(
            wire.record.operation.clone(),
            Cached {
                key: key.clone(),
                kind: kind(&op),
                activity,
                wire,
                op,
                problem,
                unavailable: false,
            },
        );
        Ok(key)
    }

    pub(super) fn mark_unavailable(&mut self, operation: &str, missing: bool) {
        if let Some(record) = self.records.get_mut(operation) {
            record.unavailable = missing;
            record.problem = Some(
                if missing {
                    "Observation is missing"
                } else {
                    "Observation is quarantined"
                }
                .into(),
            );
        }
    }

    pub(super) fn prune(
        &mut self,
        selected: Option<&str>,
        expanded: &BTreeSet<String>,
    ) -> BTreeSet<String> {
        let mut remove = self.records.len().saturating_sub(MAX_CACHED_OBSERVATIONS);
        let mut affected = BTreeSet::new();
        self.order.retain(|id| {
            let keep = self.records.get(id).is_some_and(|record| {
                remove == 0
                    || selected == Some(record.key.as_str())
                    || expanded.contains(&record.key)
            });
            if !keep {
                if let Some(record) = self.records.remove(id) {
                    if let Some(ids) = self.items.get_mut(&record.key) {
                        let _removed = ids.remove(id);
                    }
                    let _affected = affected.insert(record.key);
                    self.evicted = self.evicted.saturating_add(1);
                }
                remove = remove.saturating_sub(1);
            }
            keep
        });
        if !affected.is_empty() {
            self.items.retain(|_, ids| !ids.is_empty());
            self.stream = StreamState::default();
            for record in self.records.values() {
                if let Some(activity) = &record.activity {
                    let _replay = self.stream.insert(activity.clone());
                }
            }
        }
        affected
    }

    pub(super) fn status(&self) -> CacheStatus {
        CacheStatus {
            observations: u32::try_from(self.records.len()).unwrap_or(u32::MAX),
            evicted: self.evicted,
        }
    }

    pub(super) fn item_for_observation(&self, operation: &str) -> Option<String> {
        self.records.get(operation).map(|record| record.key.clone())
    }

    pub(super) fn item(&self, key: &str, model: &Model) -> Option<ItemView> {
        let records: Vec<_> = self
            .items
            .get(key)?
            .iter()
            .filter_map(|id| self.records.get(id))
            .collect();
        if records.is_empty() {
            return None;
        }
        Some(ItemView {
            key: key.to_owned(),
            observations: records
                .iter()
                .map(|record| observation_view(record))
                .collect(),
            blocks: blocks(&records, model, &self.stream),
            expanded: model.expanded.contains(key),
            paging: model
                .item_pages
                .get(key)
                .cloned()
                .unwrap_or_else(Paging::default),
        })
    }
}

fn observation_view(cached: &Cached) -> ObservationView {
    let activity = cached
        .activity
        .as_ref()
        .filter(|_| cached.problem.is_none());
    ObservationView {
        record: cached.wire.record.clone(),
        item: cached.key.clone(),
        kind: cached.kind,
        author: activity
            .and_then(|value| value.author)
            .map(|id| id.to_string()),
        recorder: activity.map(|value| value.recorder.to_string()),
        session: activity
            .and_then(|value| value.session)
            .map(|id| id.to_string()),
        turn: activity
            .and_then(|value| value.turn)
            .map(|id| id.to_string()),
        time_ms: activity.and_then(|value| value.time_ms),
        sequence: activity.and_then(|value| value.sequence),
        parents: if cached.problem.is_none() {
            cached.op.parent_ids().map(ToString::to_string).collect()
        } else {
            Vec::new()
        },
        causes: activity.map_or_else(Vec::new, |value| {
            value.causes.iter().map(ToString::to_string).collect()
        }),
        original: activity
            .and_then(|value| value.original.as_ref())
            .map(|original| original.operation.to_string()),
        converter: activity
            .and_then(|value| value.original.as_ref())
            .map(|original| original.converter.clone()),
        legacy: activity
            .and_then(|value| value.legacy.as_ref())
            .map(|legacy| legacy.operation.to_string()),
        detail: activity.map_or(Detail::Other, |value| detail(&value.kind)),
        preview: activity.and_then(|value| preview(&value.kind)),
        problem: cached.problem.clone(),
    }
}

fn preview(kind: &Kind) -> Option<ContentText> {
    let payload = match kind {
        Kind::Message(value) => &value.blocks.first()?.content,
        Kind::Tool(value) => &value.name,
        Kind::File(value) => &value.name,
        Kind::Session(value) => &value.label,
        Kind::Commit(value) => &value.message,
        Kind::Note(value) => &value.content,
        Kind::Author(value) => &value.label,
        Kind::Link(value) => &value.content,
        Kind::Original(value) => &value.bytes,
        Kind::Turn(_) => return None,
    };
    let Payload::Inline(bytes) = payload else {
        return None;
    };
    let complete = match kind {
        Kind::Message(message) => {
            message.blocks.len() == 1
                && message
                    .blocks
                    .first()
                    .is_some_and(|block| block.mode == activity::UpdateMode::Replace)
        }
        Kind::Tool(_)
        | Kind::File(_)
        | Kind::Session(_)
        | Kind::Commit(_)
        | Kind::Note(_)
        | Kind::Author(_)
        | Kind::Link(_)
        | Kind::Original(_)
        | Kind::Turn(_) => true,
    };
    std::str::from_utf8(bytes)
        .ok()
        .map(|text| ContentText::new(text.to_owned(), complete))
}

fn detail(kind: &Kind) -> Detail {
    match kind {
        Kind::Message(value) => Detail::Message {
            category: format!("{:?}", value.category),
            stage: format!("{:?}", value.stage),
        },
        Kind::Tool(value) => Detail::Tool {
            attempt: value.attempt.to_string(),
            channel: format!("{:?}", value.channel),
            stage: format!("{:?}", value.stage),
            parent_call: value.parent_call.map(|id| id.to_string()),
        },
        Kind::File(value) => Detail::File {
            path: value.path.0.to_string(),
            revision: value.revision.map(|id| id.to_string()),
            action: format!("{:?}", value.action),
            change: value.change.map(|change| format!("{change:?}")),
            before: value.before.and_then(|id| serde_json::to_string(&id).ok()),
            after: value.after.and_then(|id| serde_json::to_string(&id).ok()),
            caused_by: value.caused_by.map(|id| id.to_string()),
        },
        Kind::Link(value) => Detail::Link {
            from: endpoint(&value.from),
            relation: value.relation.clone(),
            to: value.to.iter().map(endpoint).collect(),
        },
        Kind::Session(_)
        | Kind::Turn(_)
        | Kind::Commit(_)
        | Kind::Note(_)
        | Kind::Author(_)
        | Kind::Original(_) => Detail::Other,
    }
}

fn endpoint(entity: &activity::Entity) -> Endpoint {
    match entity {
        activity::Entity::Operation(id) => Endpoint::Observation(id.to_string()),
        activity::Entity::Item(id) => Endpoint::Item(id.to_string()),
        activity::Entity::Git { repository, oid } => Endpoint::Git {
            repository: repository.0.to_string(),
            oid: oid.to_hex(),
        },
    }
}

fn resolve(payload: &Payload, model: &Model) -> Option<Vec<u8>> {
    match payload {
        Payload::Empty => None,
        Payload::Inline(bytes) => Some(bytes.clone()),
        Payload::Blob(reference) => {
            let id = serde_json::to_string(&reference.id).ok()?;
            model.operation_details.values().find_map(|state| {
                let OperationDetailsState::Ready(operation_details) = state else {
                    return None;
                };
                operation_details.fields.iter().find_map(|field| {
                    if field.content_id.as_ref() != Some(&id) {
                        return None;
                    }
                    let ContentValue::Available(bytes) = &field.value else {
                        return None;
                    };
                    (usize::try_from(reference.len).ok() == Some(bytes.len()))
                        .then(|| bytes.clone())
                })
            })
        }
    }
}

fn blocks(records: &[&Cached], model: &Model, stream: &StreamState) -> Vec<BlockView> {
    let mut descriptors = BTreeMap::new();
    let mut problem = records.iter().find_map(|record| record.problem.clone());
    for record in records {
        let Some(activity) = &record.activity else {
            continue;
        };
        let updates = match &activity.kind {
            Kind::Message(message) => message
                .blocks
                .iter()
                .map(|update| (update, None, None))
                .collect::<Vec<_>>(),
            Kind::Tool(tool) => tool
                .output
                .iter()
                .map(|update| {
                    (
                        update,
                        Some(tool.attempt),
                        Some(format!("{:?}", tool.channel)),
                    )
                })
                .collect(),
            Kind::Session(_)
            | Kind::Turn(_)
            | Kind::File(_)
            | Kind::Commit(_)
            | Kind::Note(_)
            | Kind::Author(_)
            | Kind::Link(_)
            | Kind::Original(_) => Vec::new(),
        };
        for (update, attempt, channel) in updates {
            let media_type =
                resolve(&update.media_type, model).and_then(|bytes| String::from_utf8(bytes).ok());
            let descriptor = descriptors
                .entry((activity.item, update.block, attempt))
                .or_insert((update.position, channel.clone(), media_type.clone()));
            if descriptor.1 != channel
                || descriptor
                    .0
                    .zip(update.position)
                    .is_some_and(|(before, after)| before != after)
                || descriptor
                    .2
                    .as_ref()
                    .zip(media_type.as_ref())
                    .is_some_and(|(before, after)| before != after)
            {
                problem = Some("Conflicting block position or output channel".into());
            }
            descriptor.0 = descriptor.0.or(update.position);
            descriptor.2 = descriptor.2.clone().or(media_type);
        }
    }
    descriptors
        .into_iter()
        .map(
            |((item, block, attempt), (position, channel, media_type))| {
                let state = problem.as_ref().map_or_else(
                    || match stream.content(item, block, attempt, |payload| resolve(payload, model))
                    {
                        Ok(Some(content)) => BlockState::Content {
                            bytes: content.bytes,
                            complete: content.complete,
                            finished: content.finished,
                            head: content.head.to_string(),
                        },
                        Ok(None) => BlockState::Unavailable("No recorded update".into()),
                        Err(activity::ReplayError::Unavailable(id)) => {
                            BlockState::Unavailable(id.to_string())
                        }
                        Err(error) => BlockState::Conflicted(error.to_string()),
                    },
                    |problem| {
                        if records.iter().any(|record| record.unavailable) {
                            BlockState::Unavailable(problem.clone())
                        } else {
                            BlockState::Conflicted(problem.clone())
                        }
                    },
                );
                BlockView {
                    block: block.to_string(),
                    position,
                    attempt: attempt.map(|id| id.to_string()),
                    channel,
                    media_type,
                    state,
                }
            },
        )
        .collect()
}
