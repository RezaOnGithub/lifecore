use crate::{Edge, Entry, EntryId, Node};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ViewId {
    pub database: Uuid,
    pub end: usize,
    pub head: Option<EntryId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DiagnosticLocation {
    pub view: ViewId,
    pub entry: Option<EntryId>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ViewError {
    #[error("requested boundary {requested} exceeds visible boundary {visible}")]
    BeyondBoundary { requested: usize, visible: usize },
    #[error("the views do not share the required history prefix")]
    DifferentHistory,
}

#[derive(Clone, Copy, Debug)]
pub struct View<'a> {
    database: Uuid,
    //A Store only the visible slice. No analyzer operation can expand into a future suffix.
    entries: &'a [Entry],
}

impl<'a> View<'a> {
    pub(crate) fn new(database: Uuid, entries: &'a [Entry]) -> Self {
        Self { database, entries }
    }
    pub fn id(self) -> ViewId {
        ViewId {
            database: self.database,
            end: self.len(),
            head: self.entries.last().map(Entry::id),
        }
    }
    pub fn len(self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(self) -> bool {
        self.entries.is_empty()
    }
    pub fn prefix(self, end: usize) -> Result<Self, ViewError> {
        let entries = self.entries.get(..end).ok_or(ViewError::BeyondBoundary {
            requested: end,
            visible: self.len(),
        })?;
        Ok(Self::new(self.database, entries))
    }
    pub fn location(self, entry: Option<EntryId>) -> Option<DiagnosticLocation> {
        if let Some(id) = entry {
            self.get(id)?;
        }
        Some(DiagnosticLocation {
            view: self.id(),
            entry,
        })
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
        Entry::Node(_) => Some(self.nodes().any(|n| n.parent == Some(id))),
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
    pub fn ancestors(self, id: EntryId) -> impl Iterator<Item = &'a Node> {
        std::iter::successors(self.node(id), move |node| {
            node.parent.and_then(|id| self.node(id))
        })
    }
    pub fn root_of(self, id: EntryId) -> Option<EntryId> {
        self.ancestors(id).last().map(|n| n.id)
    }
    pub fn fresh_successors(self, id: EntryId) -> impl Iterator<Item = &'a Node> {
        self.fresh_nodes()
            .filter(move |node| self.ancestors(node.id).any(|n| n.id == id))
    }
    pub fn relative_degree(self, a: EntryId, b: EntryId) -> Option<usize> {
        for (left, ancestor) in self.ancestors(a).skip(1).enumerate() {
            if let Some(right) = self.ancestors(b).skip(1).position(|n| n.id == ancestor.id) {
                return Some(left.max(right));
            }
        }
        None
    }
    pub fn common_prefix(self, other: View<'_>) -> Result<usize, ViewError> {
        if self.database != other.database {
            return Err(ViewError::DifferentHistory);
        }
        //A Compare identities and contents: a matching count or lifetime proves no
        //A ancestry.
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
