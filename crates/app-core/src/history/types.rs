//! Portable query and evidence contracts for Rust and generated native shells.

use crux_core::capability::Operation;
use serde::{Deserialize, Serialize};

use crate::module::EffectError;

/// Recorded category, including records without a supported activity adapter.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum ActivityKind {
    /// Conversation or editing session.
    Session,
    /// Agent execution.
    Turn,
    /// Message, reasoning, plan or summary.
    Message,
    /// Tool or terminal call.
    Tool,
    /// File interaction or revision.
    File,
    /// Git commit.
    Commit,
    /// Annotation or diagnostic.
    Note,
    /// Recorded identity.
    Author,
    /// Explicit logical connection.
    Link,
    /// Exact captured input.
    Original,
    /// Chain initialization, without an invented activity.
    Initialization,
    /// Unsupported record retained as evidence.
    Unknown,
}

/// Conjunctive recorded-fact filters; an empty kind list accepts all kinds.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct Filter {
    /// Accepted activity kinds.
    pub kinds: Vec<ActivityKind>,
    /// Full logical session identity.
    pub session: Option<String>,
    /// Full recorded author identity, distinct from recorder.
    pub author: Option<String>,
    /// Full recorder identity.
    pub recorder: Option<String>,
    /// Exact decimal path identity, not a working-tree path.
    pub path: Option<String>,
}

/// Bounded scan parameters. Continuation order does not establish chronology.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct Page {
    /// Exclusive full observation ID; never a durable subscription cursor.
    pub after: Option<String>,
    /// Maximum candidates to inspect, between 1 and 1000.
    pub limit: u32,
}

impl Default for Page {
    fn default() -> Self {
        Self {
            after: None,
            limit: 100,
        }
    }
}

/// Host operation scoped only to a caller-resolved logical chain.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct Query {
    /// Opaque chain binding resolved by the host independently of workspace UI.
    pub chain: String,
    /// Exact requested operation.
    pub action: QueryAction,
}

/// Engine reads and platform actions required by the history reducer.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum QueryAction {
    /// Scan accepted history with conjunctive semantic filters.
    History {
        /// Recorded-fact filters.
        filter: Filter,
        /// Candidate page.
        page: Page,
    },
    /// Search exact recorded bytes; gaps remain part of the result.
    Search {
        /// Case-sensitive literal UTF-8 query.
        text: String,
        /// Recorded-fact filters.
        filter: Filter,
        /// Candidate page.
        page: Page,
    },
    /// Load observations of a logical item through bounded pages.
    Item {
        /// Full logical identity.
        item: String,
        /// Candidate page.
        page: Page,
    },
    /// Fetch exact records and fields, including quarantined representations.
    Evidence {
        /// Full observation identity.
        operation: String,
    },
    /// Ask the platform to open recorded content through its capabilities.
    Open {
        /// Evidence identity, never a preview or row index.
        record: RecordRef,
        /// Native action to perform.
        target: OpenTarget,
    },
}

/// Native content action. Hosts report unavailable capabilities as errors.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum OpenTarget {
    /// Exact retained operation encoding.
    Record,
    /// Exact captured Original linked by this observation.
    Original,
    /// Recorded file revision.
    File,
    /// Recorded before/after comparison.
    Diff,
}

/// Full observation identity plus digest of one exact retained representation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct RecordRef {
    /// Full 256-bit observation ID in canonical lowercase hexadecimal.
    pub operation: String,
    /// Full BLAKE3 digest of original encoded bytes, in lowercase hexadecimal.
    pub hash: String,
}

/// Lossless engine observation using `EditChain`'s shared text/binary JSON codecs.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct Observation {
    /// Original encoding's stable reference.
    pub record: RecordRef,
    /// Complete `editchain_core::Op` JSON, not display-op or CLI scan output.
    /// This transport representation is distinct from the original encoding.
    pub operation_json: Vec<u8>,
}

/// A page retains its candidate cursor even when filtering produced no rows.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct HistoryPage {
    /// Full observations in engine query order.
    pub observations: Vec<Observation>,
    /// Last inspected observation when further candidates remain.
    pub next_after: Option<String>,
    /// Candidate count, not a count of logical items.
    pub scanned: u32,
}

/// One exact byte match, retaining evidence and field identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct SearchMatch {
    /// Complete matching observation, including item identity and attribution.
    pub observation: Observation,
    /// Matching fields in recorded order.
    pub fields: Vec<MatchRange>,
}

/// Half-open byte offsets in one recorded field, not UTF-16 or rendered offsets.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct MatchRange {
    /// Engine `ContentField` JSON, preserving indexed field selectors.
    pub field: String,
    /// Inclusive byte offset.
    pub start: u64,
    /// Exclusive byte offset.
    pub end: u64,
}

/// Search results distinguish no match from unsearchable content.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct SearchPage {
    /// Known matches only.
    pub matches: Vec<SearchMatch>,
    /// Fields for which search could not inspect complete content.
    pub unavailable: Vec<FieldEvidence>,
    /// Candidate count including nonmatches and gaps.
    pub scanned: u32,
    /// Continue even when there were zero matches.
    pub next_after: Option<String>,
}

/// Exact field availability; an empty byte vector is known empty content.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum EvidenceValue {
    /// Exact verified bytes, including binary and empty content.
    Available(Vec<u8>),
    /// Field absent or inapplicable.
    NotRecorded,
    /// Referenced content has not arrived.
    Missing,
    /// Bytes failed recorded address or length verification.
    Corrupt,
    /// The host cannot resolve the recorded identity.
    Unresolvable,
}

/// One exact field value, independently addressable for drill-down.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct FieldEvidence {
    /// Exact supporting record.
    pub record: RecordRef,
    /// Engine `ContentField` JSON, also used by search matches.
    pub field: String,
    /// Shared-codec content-ID JSON, when the field references a blob.
    pub content_id: Option<String>,
    /// Declared blob length, when recorded.
    pub declared_length: Option<u32>,
    /// Exact bytes or an explicit gap.
    pub value: EvidenceValue,
}

/// Exact archive bytes; conflicted variants are kept separately.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct RawRecord {
    /// Identity of this particular encoding.
    pub record: RecordRef,
    /// Original stored encoding, never reserialized.
    pub bytes: Vec<u8>,
}

/// Lookup status independently of field-content availability.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum EvidenceStatus {
    /// One accepted representation.
    Found,
    /// No accepted or retained representation is available.
    Missing,
    /// Quarantined identity; no variant is selected as fact.
    Conflicted,
}

/// Lossless file comparison; byte ranges are not rendered line hunks.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum Comparison {
    /// At least one complete side is unavailable.
    Unavailable,
    /// Both exact snapshots are equal.
    Identical,
    /// Replace the before range with the indicated after bytes.
    Changed {
        /// Before-side start.
        before_start: u64,
        /// Before-side end.
        before_end: u64,
        /// After-side start.
        after_start: u64,
        /// After-side end.
        after_end: u64,
    },
}

/// On-demand evidence shared by all client detail surfaces.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct Evidence {
    /// Requested full observation ID, including for missing records.
    pub operation: String,
    /// Missing and conflicted identities stay explicit.
    pub status: EvidenceStatus,
    /// Accepted observation only; never populated for a conflict.
    pub observation: Option<Observation>,
    /// All retained original encodings, including quarantined variants.
    pub records: Vec<RawRecord>,
    /// Exact content and explicit field gaps.
    pub fields: Vec<FieldEvidence>,
    /// Present for file revisions; absent snapshots remain unavailable.
    pub comparison: Option<Comparison>,
}

/// Query response union checked against the pending operation by the reducer.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum QueryResult {
    /// History or logical-item scan.
    History(HistoryPage),
    /// Search scan including gaps.
    Search(SearchPage),
    /// Exact observation evidence.
    Evidence(Evidence),
    /// Platform confirmed an open action.
    Opened,
}

/// Result returned by typed Rust hosts.
pub type QueryOutput = Result<QueryResult, EffectError>;

/// Named wire result matching the binary layout of [`QueryOutput`].
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum QueryResponse {
    /// Successful host result.
    Ok(QueryResult),
    /// Presentable host failure.
    Err(EffectError),
}

impl Operation for Query {
    type Output = QueryOutput;
}
