use crate::{
    Body, Edge, Entry, EntryId, Node,
    index::{CoreIndexes, IndexTemplate, Indexes, TemplateIndex, TemplateIndexes},
};
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
    // The graph's core indexes, which may cover entries past this view, so every
    // query narrows them to `entries` through `Indexes`. A view without them
    // answers every query by its reference scan (SPEC §9.1).
    indexes: Option<&'a CoreIndexes>,
    // The index templates a run built over its head view, which this view is a
    // prefix of. Read only beside the core indexes, whose roots and children
    // they rely on.
    templates: Option<&'a TemplateIndexes>,
}

// Debug output omits the indexes and templates: their values can name
// positions past this view, and whether a view carries them is not part of
// what it shows.
impl std::fmt::Debug for View<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self {
            database,
            entries,
            indexes: _,
            templates: _,
        } = self;
        f.debug_struct("View")
            .field("database", database)
            .field("entries", entries)
            .finish()
    }
}

// One of two iterators, such as the indexed and the reference answer to a
// query, as one type.
pub(crate) enum Either<I, S> {
    Left(I),
    Right(S),
}

impl<T, I: Iterator<Item = T>, S: Iterator<Item = T>> Iterator for Either<I, S> {
    type Item = T;
    fn next(&mut self) -> Option<T> {
        match self {
        Self::Left(i) => i.next(),
        Self::Right(s) => s.next(),
        }
    }
}

impl<T, I: DoubleEndedIterator<Item = T>, S: DoubleEndedIterator<Item = T>> DoubleEndedIterator
    for Either<I, S>
{
    fn next_back(&mut self) -> Option<T> {
        match self {
        Self::Left(i) => i.next_back(),
        Self::Right(s) => s.next_back(),
        }
    }
}

impl<'a> View<'a> {
    pub(crate) fn new(
        database: Uuid,
        entries: &'a [Entry],
        indexes: Option<&'a CoreIndexes>,
    ) -> Self {
        debug_assert!(indexes.is_none_or(|indexes| indexes.len() >= entries.len()));
        Self {
            database,
            entries,
            indexes,
            templates: None,
        }
    }
    /// The same view without indexes or templates: every query answers by its
    /// reference scan, with the same results (SPEC §9.1).
    pub fn without_indexes(self) -> Self {
        Self {
            indexes: None,
            templates: None,
            ..self
        }
    }
    // The same view with the core indexes only.
    pub(crate) fn without_templates(self) -> Self {
        Self {
            templates: None,
            ..self
        }
    }
    // The same view with `indexes`, which must have been built from entries
    // this view's entries are a prefix of.
    pub(crate) fn with_indexes<'b>(self, indexes: &'b CoreIndexes) -> View<'b>
    where
        'a: 'b,
    {
        View::new(self.database, self.entries, Some(indexes))
    }
    // The same view with `templates`, which must have been built over entries
    // this view's entries are a prefix of.
    pub(crate) fn with_templates<'b>(self, templates: &'b TemplateIndexes) -> View<'b>
    where
        'a: 'b,
    {
        debug_assert!(templates.len() >= self.len());
        View {
            templates: Some(templates),
            ..self
        }
    }
    pub(crate) fn core_indexes(self) -> Option<&'a CoreIndexes> {
        self.indexes
    }
    /// The core indexes narrowed to this view, if it carries them.
    pub fn indexes(self) -> Option<Indexes<'a>> {
        self.indexes
            .map(|indexes| Indexes::new(self.entries, indexes))
    }
    /// The index built for `template`, narrowed to this view, if the view
    /// carries it (SPEC §9.1). Without it, `template.scan(view)` answers the
    /// same queries.
    pub fn template(self, template: &IndexTemplate) -> Option<TemplateIndex<'a>> {
        TemplateIndex::new(self.entries, self.indexes?, self.templates?, template)
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
        Ok(Self { entries, ..self })
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
    pub(crate) fn entry_slice(self) -> &'a [Entry] {
        self.entries
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
    // A node is stale exactly when one of its children is visible; an edge when
    // either endpoint is.
    pub fn is_stale(self, id: EntryId) -> Option<bool> {
        match self.get(id)? {
        Entry::Node(_) => Some(match self.indexes() {
        Some(indexes) => indexes.has_child(id)?,
        None => self.children(id).next().is_some(),
        }),
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
        match self.indexes() {
        Some(indexes) => Either::Left(indexes.edges_of(id)),
        None => Either::Right(self.edges().filter(move |e| e.from == id || e.to == id)),
        }
    }
    pub fn children(self, id: EntryId) -> impl Iterator<Item = &'a Node> {
        match self.indexes() {
        Some(indexes) => Either::Left(indexes.children(id)),
        None => Either::Right(self.nodes().filter(move |n| n.predecessor == Some(id))),
        }
    }
    /// The nodes of this view whose type name is `ty_name`, current or not.
    pub fn nodes_of_type(self, ty_name: &str) -> impl DoubleEndedIterator<Item = &'a Node> {
        match self.indexes() {
        Some(indexes) => Either::Left(indexes.nodes_of_type(ty_name)),
        None => Either::Right(self.nodes().filter(move |n| n.ty_name == ty_name)),
        }
    }
    /// The edges of this view labelled `ty_name`, current or not.
    pub fn edges_labelled(self, ty_name: &str) -> impl DoubleEndedIterator<Item = &'a Edge> {
        match self.indexes() {
        Some(indexes) => Either::Left(indexes.edges_labelled(ty_name)),
        None => Either::Right(self.edges().filter(move |e| e.ty_name == ty_name)),
        }
    }
    /// The nodes of this view with exactly this type name, predecessor, and
    /// payload, current or not: the candidates of the duplicate-node rule
    /// (SPEC §7.1).
    pub fn nodes_with_content<'q>(
        self,
        ty_name: &'q str,
        predecessor: Option<EntryId>,
        payload: &'q Body,
    ) -> impl DoubleEndedIterator<Item = &'a Node> + use<'a, 'q> {
        match self.indexes() {
        Some(indexes) => Either::Left(indexes.nodes_with_content(ty_name, predecessor, payload)),
        None => Either::Right(self.nodes().filter(move |n| {
            n.ty_name == ty_name && n.predecessor == predecessor && n.payload == *payload
        })),
        }
    }
    /// The edge of this view from `from` to `to` labelled `ty_name`, current or
    /// not; validation admits at most one (SPEC §7.1).
    pub fn edge_between(self, from: EntryId, to: EntryId, ty_name: &str) -> Option<&'a Edge> {
        match self.indexes() {
        Some(indexes) => indexes.edge_between(from, to, ty_name),
        None => self
            .edges()
            .find(|e| e.from == from && e.to == to && e.ty_name == ty_name),
        }
    }
    // Roots and current endpoints need an inclusive walk; public ancestry starts
    // at the parent so a node is never its own ancestor.
    fn lineage(self, id: EntryId) -> impl Iterator<Item = &'a Node> {
        std::iter::successors(self.node(id), move |node| {
            node.predecessor.and_then(|id| self.node(id))
        })
    }
    pub fn ancestors(self, id: EntryId) -> impl Iterator<Item = &'a Node> {
        self.lineage(id).skip(1)
    }
    pub fn root_of(self, id: EntryId) -> Option<EntryId> {
        match self.indexes() {
        Some(indexes) => indexes.root_of(id),
        None => self.lineage(id).last().map(|n| n.id),
        }
    }
    pub fn fresh_successors(self, id: EntryId) -> impl Iterator<Item = &'a Node> {
        match self.indexes() {
        // The start, when current, and its current descendants, in append order.
        Some(indexes) => Either::Left(
            self.node(id)
                .into_iter()
                .chain(indexes.descendants(id))
                .filter(move |node| indexes.has_child(node.id) == Some(false)),
        ),
        None => Either::Right(
            self.fresh_nodes()
                .filter(move |node| self.lineage(node.id).any(|n| n.id == id)),
        ),
        }
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
