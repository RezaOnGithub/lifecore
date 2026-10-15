use crate::{Edge, Entry, EntryId, Node};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ViewId {
    pub database: Uuid,
    #[serde(with = "crate::decimal")]
    pub end_exclusive: usize,
    pub head: Option<EntryId>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ViewError {
    #[error("requested boundary {requested} exceeds visible boundary {visible}")]
    BeyondBoundary { requested: usize, visible: usize },
    #[error("the views do not share the required history prefix")]
    DifferentHistory,
}

#[derive(Clone, Copy)]
pub struct View<'a> {
    database: Uuid,
    // Store only the visible slice. No analyzer operation can expand into a future suffix.
    entries: &'a [Entry],
    // The staleness index narrowed to the same slice. A stored first-child position
    // may lie beyond this view, so queries only compare it with `len()`.
    first_child: &'a [Option<usize>],
}

// Debug output omits the staleness index: its values can name positions past
// this view, and printing them would expose the future suffix.
impl std::fmt::Debug for View<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self {
            database,
            entries,
            first_child: _,
        } = self;
        f.debug_struct("View")
            .field("database", database)
            .field("entries", entries)
            .finish()
    }
}

impl<'a> View<'a> {
    pub(crate) fn new(
        database: Uuid,
        entries: &'a [Entry],
        first_child: &'a [Option<usize>],
    ) -> Self {
        Self {
            database,
            entries,
            first_child: &first_child[..entries.len()],
        }
    }
    pub fn id(self) -> ViewId {
        ViewId {
            database: self.database,
            end_exclusive: self.len(),
            head: self.entries.last().map(Entry::id),
        }
    }
    pub fn len(self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(self) -> bool {
        self.entries.is_empty()
    }
    // A view contains positions [0, end_exclusive); a prefix can only narrow it.
    pub fn prefix(self, end_exclusive: usize) -> Result<Self, ViewError> {
        let entries = self
            .entries
            .get(..end_exclusive)
            .ok_or(ViewError::BeyondBoundary {
                requested: end_exclusive,
                visible: self.len(),
            })?;
        Ok(Self::new(self.database, entries, self.first_child))
    }
    pub fn get(self, id: EntryId) -> Option<&'a Entry> {
        self.entries
            .get(id.position)
            .filter(|entry| entry.id() == id)
    }
    pub fn at(self, position: usize) -> Option<&'a Entry> {
        self.entries.get(position)
    }
    pub fn node(self, id: EntryId) -> Option<&'a Node> {
        self.get(id)?.as_node()
    }
    pub fn edge(self, id: EntryId) -> Option<&'a Edge> {
        self.get(id)?.as_edge()
    }
    pub fn entries(self) -> impl DoubleEndedIterator<Item = &'a Entry> {
        self.entries.iter()
    }
    pub fn nodes(self) -> impl DoubleEndedIterator<Item = &'a Node> {
        self.entries.iter().filter_map(Entry::as_node)
    }
    pub fn edges(self) -> impl DoubleEndedIterator<Item = &'a Edge> {
        self.entries.iter().filter_map(Entry::as_edge)
    }
    pub fn is_stale(self, id: EntryId) -> Option<bool> {
        match self.get(id)? {
        // A node is stale exactly when its first child is visible.
        Entry::Node(_) => {
            Some(self.first_child[id.position].is_some_and(|child| child < self.len()))
        }
        Entry::Edge(e) => Some(self.is_stale(e.from)? || self.is_stale(e.to)?),
        }
    }
    pub fn fresh_nodes(self) -> impl Iterator<Item = &'a Node> {
        self.nodes()
            .filter(move |n| self.is_stale(n.id) == Some(false))
    }
    pub fn fresh_edges(self) -> impl Iterator<Item = &'a Edge> {
        self.edges()
            .filter(move |e| self.is_stale(e.id) == Some(false))
    }
    pub fn edges_of(self, id: EntryId) -> impl Iterator<Item = &'a Edge> {
        self.edges().filter(move |e| e.from == id || e.to == id)
    }
    pub fn children(self, id: EntryId) -> impl Iterator<Item = &'a Node> {
        self.nodes().filter(move |n| n.parent == Some(id))
    }
    // Roots and current endpoints need an inclusive walk; public ancestry starts
    // at the parent so a node is never its own ancestor.
    fn lineage(self, id: EntryId) -> impl Iterator<Item = &'a Node> {
        std::iter::successors(self.node(id), move |node| {
            node.parent.and_then(|id| self.node(id))
        })
    }
    pub fn ancestors(self, id: EntryId) -> impl Iterator<Item = &'a Node> {
        self.lineage(id).skip(1)
    }
    pub fn root_of(self, id: EntryId) -> Option<EntryId> {
        self.lineage(id).last().map(|n| n.id)
    }
    pub fn fresh_successors(self, id: EntryId) -> impl Iterator<Item = &'a Node> {
        self.fresh_nodes()
            .filter(move |node| self.lineage(node.id).any(|n| n.id == id))
    }
    pub fn relative_degree(self, a: EntryId, b: EntryId) -> Option<usize> {
        for (left, ancestor) in self.ancestors(a).enumerate() {
            if let Some(right) = self.ancestors(b).position(|n| n.id == ancestor.id) {
                return Some(left.max(right));
            }
        }
        None
    }
    pub fn common_prefix(self, other: View<'_>) -> Result<usize, ViewError> {
        if self.database != other.database {
            return Err(ViewError::DifferentHistory);
        }
        // Compare identities and contents: a matching count or lifetime proves no
        // ancestry.
        Ok(self
            .entries
            .iter()
            .zip(other.entries)
            .take_while(|(a, b)| a == b)
            .count())
    }
    pub fn extends(self, base: View<'_>) -> Result<(), ViewError> {
        if self.common_prefix(base)? == base.len() {
            Ok(())
        } else {
            Err(ViewError::DifferentHistory)
        }
    }
}
