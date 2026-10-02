//! Typed graph identity and resolved edges for one projection stage.

use std::collections::HashMap;

use editchain_core::{GitCommitKey, OpId};

use crate::taxonomy::ChainState;
use crate::RelationKind;

/// Stable graph identity, with repository identity retained for Git commits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NodeKey {
    /// An accepted source operation or a group anchored on that operation.
    Op(OpId),
    /// A commit in one repository.
    Git(GitCommitKey),
}

impl NodeKey {
    /// Parse a display key at an external compatibility boundary.
    #[must_use]
    pub fn from_display_str(value: &str) -> Option<Self> {
        OpId::from_display_str(value)
            .map(Self::Op)
            .or_else(|| GitCommitKey::from_display_str(value).map(Self::Git))
    }
}

impl std::fmt::Display for NodeKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Op(id) => id.fmt(formatter),
            Self::Git(key) => key.fmt(formatter),
        }
    }
}

/// A structural label supported by source evidence on a resolved parent edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRelation {
    /// Parent in this exact graph stage.
    pub parent: NodeKey,
    /// Provider-neutral meaning of the edge.
    pub kind: RelationKind,
    /// Relationship-note IDs, or producing source-operation IDs for Git links.
    pub evidence: Vec<OpId>,
}

/// One immutable graph stage in display order. Every parent belongs to it.
///
/// Scheduling, Activity filtering, and geometry resolve ancestry through the
/// same typed node contract. A completed view retains this table, so paging
/// and relation labels do not re-interpret source envelopes.
#[derive(Debug)]
pub struct ResolvedGraph {
    keys: Vec<NodeKey>,
    rows: HashMap<NodeKey, ResolvedGraphRow>,
}

#[derive(Debug)]
pub(crate) struct ResolvedGraphRow {
    pub(crate) parents: Vec<NodeKey>,
    pub(crate) relations: Vec<ResolvedRelation>,
    pub(crate) chain_state: ChainState,
}

impl ResolvedGraph {
    pub(crate) fn new(keys: Vec<NodeKey>, rows: HashMap<NodeKey, ResolvedGraphRow>) -> Self {
        Self { keys, rows }
    }

    /// Nodes in the exact order used by layout.
    #[must_use]
    pub fn keys(&self) -> &[NodeKey] {
        &self.keys
    }

    /// Final deduplicated parents, in source precedence order.
    #[must_use]
    pub fn parents(&self, key: NodeKey) -> &[NodeKey] {
        self.rows.get(&key).map_or(&[], |row| &row.parents)
    }

    /// Structural labels whose endpoints both exist in this graph.
    #[must_use]
    pub fn relations(&self, key: NodeKey) -> &[ResolvedRelation] {
        self.rows.get(&key).map_or(&[], |row| &row.relations)
    }

    /// Read the source-derived chain state, defaulting to active for an unknown key.
    #[must_use]
    pub fn chain_state(&self, key: NodeKey) -> ChainState {
        self.rows
            .get(&key)
            .map_or(ChainState::Active, |row| row.chain_state)
    }
}
