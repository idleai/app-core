//! Client-facing semantic models, independent of rendering and row positions.

use serde::{Deserialize, Serialize};

use super::{ActivityKind, ContentText, Evidence, FieldEvidence, Filter, MatchRange, RecordRef};
use crate::module::EffectError;

/// Request state preserves already loaded data during paging and retry.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum RequestState {
    /// No request has run.
    #[default]
    Idle,
    /// Awaiting the host.
    Loading,
    /// The last request completed.
    Ready,
    /// The last request failed; cached data remains available.
    Failed(EffectError),
}

/// Continuation and progress for a bounded engine scan.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct Paging {
    /// Request status, independent of cached rows.
    pub state: RequestState,
    /// Exclusive observation-ID cursor, never a subscription cursor.
    pub next_after: Option<String>,
    /// Total candidates inspected by this scan.
    pub scanned: u64,
    /// True only after a successful page has no continuation.
    pub exhausted: bool,
}

/// Logical selection with a separately chosen observation for evidence.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct Selected {
    /// Stable logical item key. Unknown legacy records use their observation ID.
    pub item: Option<String>,
    /// Exact observation explicitly selected by the user, if any.
    pub observation: Option<String>,
}

/// The complete history surface state for one client.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[facet(rename = "HistoryViewModel")]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct ViewModel {
    /// Selected logical chain binding.
    pub chain: Option<String>,
    /// Current recorded-fact filters.
    pub filter: Filter,
    /// Stable interaction selection across pages, filters and refreshes.
    pub selected: Selected,
    /// Expanded logical items, independent of mounting and coordinates.
    pub expanded: Vec<String>,
    /// Items encountered in this history scan, in first-encounter engine order.
    pub items: Vec<ItemView>,
    /// Selected item remains available even when it is outside the filtered page.
    pub selected_item: Option<ItemView>,
    /// Bounded history scan status.
    pub paging: Paging,
    /// Independent search session and cursor.
    pub search: SearchView,
    /// All cached evidence states, keyed by observation identity.
    pub evidence: Vec<EvidenceView>,
    /// Most recent native-open action status.
    pub open: RequestState,
    /// Bounded cache status; evicted observations can be loaded again by item.
    pub cache: CacheStatus,
}

/// Observation cache accounting, with selected and expanded items pinned.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct CacheStatus {
    /// Number of currently retained full observations.
    pub observations: u32,
    /// Observations evicted since the last context reset or refresh.
    pub evicted: u64,
}

/// One logical object and all of its currently cached observations.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct ItemView {
    /// Full logical identity; never a title, index or shortened hash.
    pub key: String,
    /// All cached immutable observations, without choosing a latest hash.
    pub observations: Vec<ObservationView>,
    /// Reconstructed message blocks and distinct tool attempts/channels.
    pub blocks: Vec<BlockView>,
    /// Whether the client expanded this item.
    pub expanded: bool,
    /// Explicit full-item scan progress; page presence alone proves no completeness.
    pub paging: Paging,
}

/// One observation's recorded attribution, causal links and evidence identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct ObservationView {
    /// Stable reference to the exact underlying record.
    pub record: RecordRef,
    /// Logical object updated by this observation.
    pub item: String,
    /// Recorded category or explicit unsupported/initialization marker.
    pub kind: ActivityKind,
    /// Recorded author, if known.
    pub author: Option<String>,
    /// Capturing integration, if known.
    pub recorder: Option<String>,
    /// Explicit session binding.
    pub session: Option<String>,
    /// Explicit turn binding.
    pub turn: Option<String>,
    /// Recorded wall time, not inferred import time.
    pub time_ms: Option<u64>,
    /// Recorded sequence within a recorder, never inferred from hashes.
    pub sequence: Option<u64>,
    /// All physical causal parents, including beyond the legacy two-parent envelope.
    pub parents: Vec<String>,
    /// Logical causes remain distinct from physical parents and Links.
    pub causes: Vec<String>,
    /// Original evidence observation referenced by a converter.
    pub original: Option<String>,
    /// Converter identity retained when an Original is referenced.
    pub converter: Option<String>,
    /// Old address retained by a physical schema conversion.
    pub legacy: Option<String>,
    /// Typed data needed by message, tool, revision and graph detail surfaces.
    pub detail: Detail,
    /// Explicitly bounded display preview, never the complete evidence contract.
    pub preview: Option<ContentText>,
    /// Invalid, missing or quarantined evidence associated with this observation.
    pub problem: Option<String>,
}

/// Kind-specific semantics; full unmodified metadata remains in evidence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum Detail {
    /// No additional row-level semantics.
    Other,
    /// Message category and recorded lifecycle.
    Message {
        /// Text, Reasoning, Plan or Summary.
        category: String,
        /// Recorded lifecycle, not guessed completion.
        stage: String,
    },
    /// A call attempt, preserving its distinct stream.
    Tool {
        /// Full attempt identity.
        attempt: String,
        /// Recorded output channel.
        channel: String,
        /// Recorded lifecycle.
        stage: String,
        /// Known wrapper call.
        parent_call: Option<String>,
    },
    /// A recorded revision or exposure observation.
    File {
        /// Exact decimal path identity.
        path: String,
        /// Recorded revision identity, when known.
        revision: Option<String>,
        /// Recorded action, including exposure/read events.
        action: String,
        /// Recorded proposed/applied state.
        change: Option<String>,
        /// Exact recorded content-ID JSON.
        before: Option<String>,
        /// Exact recorded content-ID JSON.
        after: Option<String>,
        /// Explicit causing call, when known.
        caused_by: Option<String>,
    },
    /// A logical relationship, independent of graph geometry and causal parents.
    Link {
        /// Explicit originating endpoint.
        from: Endpoint,
        /// Recorded relation, without controller interpretation.
        relation: String,
        /// Explicit destination endpoints.
        to: Vec<Endpoint>,
    },
}

/// Graph endpoint identity retaining its recorded domain.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum Endpoint {
    /// Physical immutable observation.
    Observation(String),
    /// Logical object.
    Item(String),
    /// Repository-scoped Git object.
    Git {
        /// Exact decimal repository identity.
        repository: String,
        /// Full object identity.
        oid: String,
    },
}

/// Replayed content state from the engine's immutable Append/Replace model.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct BlockView {
    /// Full logical block identity.
    pub block: String,
    /// Recorded block position, if supplied.
    pub position: Option<u32>,
    /// Tool attempt identity; absent for message blocks.
    pub attempt: Option<String>,
    /// Recorded output channel, absent for message blocks.
    pub channel: Option<String>,
    /// Recorded UTF-8 MIME type when available; absence does not imply text.
    pub media_type: Option<String>,
    /// Reconstructed content or explicit uncertainty.
    pub state: BlockState,
}

/// Replayed bytes are never labelled complete when a predecessor is missing.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum BlockState {
    /// Known bytes, with prefix completeness and lifecycle separately reported.
    Content {
        /// Exact reconstructed bytes; binary data remains binary.
        bytes: Vec<u8>,
        /// The prefix is established by a recorded replacement.
        complete: bool,
        /// A terminal lifecycle was explicitly recorded.
        finished: bool,
        /// Last contributing observation.
        head: String,
    },
    /// Required payload bytes have not been resolved.
    Unavailable(String),
    /// Divergent branches, quarantined records or invalid predecessor links.
    Conflicted(String),
}

/// Search state independent of row locations, scroll requests or mounted content.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct SearchView {
    /// Exact submitted literal; whitespace is meaningful.
    pub text: String,
    /// Scan progress, including continuation through zero-hit pages.
    pub paging: Paging,
    /// Match order supplied by the engine.
    pub matches: Vec<MatchView>,
    /// Unsearched fields prevent a false claim of complete absence.
    pub unavailable: Vec<FieldEvidence>,
    /// Index into matches, with no implied rendered row position.
    pub cursor: Option<u32>,
}

/// One search destination identified by logical object and exact evidence.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct MatchView {
    /// Stable logical target.
    pub item: String,
    /// Supporting observation and encoding.
    pub record: RecordRef,
    /// Exact field byte offsets for client highlighting.
    pub fields: Vec<MatchRange>,
}

/// Cached on-demand evidence, independently retryable.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[expect(
    clippy::unsafe_derive_deserialize,
    reason = "Facet generates unsafe reflection helpers; these fields have no safety invariants"
)]
pub struct EvidenceView {
    /// Full requested observation ID.
    pub operation: String,
    /// Loading/error/evidence state.
    pub state: EvidenceState,
}

/// Evidence request state with concrete generated native payload codecs.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize, facet::Facet)]
#[repr(u8)]
pub enum EvidenceState {
    /// No request has run.
    #[default]
    Idle,
    /// Awaiting host evidence.
    Loading,
    /// Exact evidence, including missing or quarantined records.
    Ready(Evidence),
    /// Presentable host error, available for retry.
    Failed(EffectError),
}
