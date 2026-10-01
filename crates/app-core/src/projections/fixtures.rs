//! Development-only schema-three fixtures, never a production controller contract.
//!
//! The `app-core/projection-fixture/v1` payload is intentionally owned by this
//! adapter. f11 supplies its own versioned payloads and production mapper.

use editchain_core::{
    FrontierSet, Op, OpId, Payload,
    activity::{
        ContentUpdate, Coverage, Entity, Field, ItemId, Kind, Link, Message, MessageKind, Note,
        NoteKind, Operation, Stage, UpdateMode,
    },
};
use editchain_engine::{
    Engine,
    queries::{ChainQueries, ContentField, ContentValue},
};
use idle_protocol::v1::{
    identity::{ProjectionCount, Timestamp},
    projections::{
        FreshnessStatus, ProjectionAvailability, ProjectionFreshness, ProjectionInput,
        ProjectionKind, ProjectionRow,
    },
};
use serde::{Deserialize, Serialize};

use super::{
    adapter::error,
    engine::{ProjectionMapper, ProjectionRead, related_references, source_reference},
};
use crate::module::EffectError;

const FORMAT: &str = "app-core/projection-fixture/v1";

#[derive(Debug, Deserialize, Serialize)]
struct FixtureNote {
    format: String,
    destination: ProjectionKind,
    key: String,
    title: String,
    status: String,
}

/// Small deterministic identities for development chains, always serialized in full.
#[must_use]
pub fn id(value: u8) -> OpId {
    OpId::from_bytes([value; 32])
}

fn operation(number: u8, item: u8, kind: Kind) -> Result<Op, EffectError> {
    let mut record = Operation::new(id(number), ItemId(id(item)), ItemId(id(201)), kind);
    record.author = Some(ItemId(id(202)));
    record.session = Some(ItemId(id(203)));
    record.time_ms = Some(1000);
    record.into_op().map_err(error)
}

/// Valid Note, Link and Message/Summary records with separate logical identities.
///
/// # Errors
/// Returns schema validation or fixture payload encoding failures.
pub fn operations() -> Result<Vec<Op>, EffectError> {
    let specs = [
        (
            10,
            110,
            ProjectionKind::Task,
            "task/checks",
            "Run workspace checks",
            "active",
        ),
        (
            20,
            120,
            ProjectionKind::Error,
            "error/network",
            "Dependency fetch failed",
            "open",
        ),
        (
            30,
            130,
            ProjectionKind::Triage,
            "triage/retry",
            "Review retry policy",
            "unreviewed",
        ),
        (
            40,
            140,
            ProjectionKind::NeedInput,
            "input/provider",
            "Choose a provider",
            "waiting",
        ),
    ];
    let mut result = Vec::new();
    for (number, item, destination, key, title, status) in specs {
        let content = serde_json::to_vec(&FixtureNote {
            format: FORMAT.into(),
            destination,
            key: key.into(),
            title: title.into(),
            status: status.into(),
        })
        .map_err(error)?;
        result.push(operation(
            number,
            item,
            Kind::Note(Note {
                category: NoteKind::Comment,
                targets: vec![id(60)],
                items: vec![ItemId(id(160))],
                version: 1,
                content: Payload::Inline(content),
                code: Payload::Empty,
            }),
        )?);
    }
    let mut link = Operation::new(
        id(50),
        ItemId(id(150)),
        ItemId(id(201)),
        Kind::Link(Link {
            from: Entity::Item(ItemId(id(110))),
            relation: "fixture/context".into(),
            to: vec![Entity::Operation(id(60)), Entity::Item(ItemId(id(120)))],
            content: Payload::Inline(b"Separate logical connection".to_vec()),
        }),
    );
    link.parents = vec![id(10), id(20), id(30)];
    result.push(link.into_op().map_err(error)?);
    result.push(operation(
        60,
        160,
        Kind::Message(Message {
            category: MessageKind::Summary,
            stage: Stage::Snapshot,
            audience: Vec::new(),
            blocks: vec![ContentUpdate {
                block: ItemId(id(161)),
                position: Some(0),
                mode: UpdateMode::Replace,
                previous: None,
                media_type: Payload::Inline(b"text/plain".to_vec()),
                content: Payload::Inline(
                    b"Checks are pending. A provider choice is needed.\n".to_vec(),
                ),
            }],
            coverage: Some(Coverage {
                operations: vec![id(10), id(20), id(30), id(40)],
                frontiers: FrontierSet::default(),
                window: None,
                anchors: Payload::Empty,
            }),
            outcome: None,
        }),
    )?);
    Ok(result)
}

/// Seed only a caller-supplied development/test chain through validated engine writes.
///
/// # Errors
/// Returns schema, encoding or storage failures.
pub fn seed(engine: &Engine) -> Result<(), EffectError> {
    for record in operations()? {
        let _admission = engine.append(&record).map_err(error)?;
    }
    Ok(())
}

/// Mapper for the explicitly named fixture payload only.
#[derive(Clone, Copy, Debug, Default)]
pub struct FixtureMapper;

impl ProjectionMapper for FixtureMapper {
    fn inputs(
        &self,
        queries: &ChainQueries,
        read: &ProjectionRead,
    ) -> Result<Vec<ProjectionInput>, EffectError> {
        let mut inputs: Vec<_> = [
            ProjectionKind::Task,
            ProjectionKind::Error,
            ProjectionKind::Triage,
            ProjectionKind::NeedInput,
        ]
        .into_iter()
        .map(|kind| ProjectionInput {
            kind,
            freshness: ProjectionFreshness {
                status: FreshnessStatus::Current,
                generated_at: Some(Timestamp(1000)),
                checkpoint: Some("fixture/snapshot-v1".into()),
            },
            availability: if read.gaps.is_empty() {
                ProjectionAvailability::Complete
            } else {
                ProjectionAvailability::Partial
            },
            total: None,
            rows: Vec::new(),
            gaps: read.gaps.clone(),
        })
        .collect();
        for record in &read.records {
            let Some(activity) = Operation::view(&record.entry.operation) else {
                continue;
            };
            let Kind::Note(note) = &activity.kind else {
                continue;
            };
            if note.version != 1 {
                continue;
            }
            let Some(bytes) =
                record
                    .fields
                    .iter()
                    .find_map(|field| match (&field.field, &field.value) {
                        (ContentField::Record(Field::Content), ContentValue::Available(bytes)) => {
                            Some(bytes)
                        }
                        _ => None,
                    })
            else {
                continue;
            };
            let Ok(payload) = serde_json::from_slice::<FixtureNote>(bytes) else {
                continue;
            };
            if payload.format != FORMAT {
                continue;
            }
            if let Some(input) = inputs
                .iter_mut()
                .find(|input| input.kind == payload.destination)
            {
                input.rows.push(ProjectionRow {
                    key: payload.key,
                    title: payload.title,
                    summary: None,
                    status: Some(payload.status),
                    labels: vec!["fixture".into()],
                    sources: vec![source_reference(&record.entry)],
                    related: related_references(queries, read, &record.entry)?,
                });
            }
        }
        for input in &mut inputs {
            if input.availability == ProjectionAvailability::Complete {
                input.total = Some(ProjectionCount(
                    u64::try_from(input.rows.len()).map_err(error)?,
                ));
            }
        }
        Ok(inputs)
    }
}
