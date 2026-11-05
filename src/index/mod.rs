// The core indexes K1 to K8 of SPEC §9.1, and the index templates in
// `template`. Every core index caches a query whose reference scan is the
// unindexed path of the matching `View` method; `View` falls back to that
// scan whenever a view carries no indexes, and the property tests hold the
// two to each other at every boundary.

use crate::{Body, Edge, Entry, EntryId, Node, typing::Declaration};
use std::{
    collections::HashMap,
    fmt,
    hash::{BuildHasher, BuildHasherDefault, DefaultHasher, Hash},
};

mod template;
pub(crate) use template::TemplateIndexes;
pub use template::{End, IndexTemplate, TemplateIndex, TemplateScan};

// Derived from the entries alone and never serialized. Every list holds
// positions in append order, so a view narrows it with one `partition_point`;
// nothing is removed or rewritten after an append, so one value serves every
// prefix view. Only entries validated against the prefix before them are
// pushed, so an invalid entry never reaches an earlier boundary.
//
// Equality is for the debug assertions that compare a rebuilt value with the
// maintained one; no public type exposes it.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct CoreIndexes {
    // K1: one slot per entry, the positions of that node's direct successors.
    // The first one decides staleness. Edges hold empty lists.
    children: Vec<Vec<usize>>,
    // K2: one slot per entry, the positions of the edges incident to that
    // node, in either direction.
    incident: Vec<Vec<usize>>,
    // K3 and K4: node positions per type name and edge positions per label.
    nodes_by_type: HashMap<String, Vec<usize>>,
    edges_by_label: HashMap<String, Vec<usize>>,
    // K5 and K6: positions per content hash. A hash only narrows the search;
    // every lookup compares complete values, so a collision cannot merge two
    // contents.
    node_content: HashMap<u64, Vec<usize>>,
    edge_triple: HashMap<u64, Vec<usize>>,
    // K7: one slot per entry, the position of that node's root, fixed at
    // append. An edge holds its own position, which no query reads.
    roots: Vec<usize>,
    // K8: the parse of every `Type` node, parallel to its K3 list.
    declarations: Vec<Declaration>,
}

// One deterministic hasher for both content indexes; the hashes are never
// observable, so determinism only keeps rebuilt and maintained values equal.
fn content_hash(value: impl Hash) -> u64 {
    #[cfg(test)]
    if COLLIDING.with(std::cell::Cell::get) {
        return 0;
    }
    BuildHasherDefault::<DefaultHasher>::default().hash_one(value)
}

// Tests make every content hash collide on their own thread, as the
// recorder's tests do with a colliding hasher, so that every K5 and K6 lookup
// must tell contents apart by complete values. Indexes built in this mode
// are only queried in it.
#[cfg(test)]
thread_local! {
    static COLLIDING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(test)]
pub(crate) struct Colliding(());

#[cfg(test)]
impl Colliding {
    pub(crate) fn start() -> Self {
        COLLIDING.with(|colliding| colliding.set(true));
        Self(())
    }
}

#[cfg(test)]
impl Drop for Colliding {
    fn drop(&mut self) {
        COLLIDING.with(|colliding| colliding.set(false));
    }
}

fn node_hash(ty_name: &str, predecessor: Option<EntryId>, payload: &Body) -> u64 {
    content_hash((ty_name, predecessor, payload))
}

fn edge_hash(from: EntryId, to: EntryId, ty_name: &str) -> u64 {
    content_hash((from, to, ty_name))
}

// Appends to the list of an existing name without allocating its key again.
fn push_keyed(lists: &mut HashMap<String, Vec<usize>>, name: &str, position: usize) {
    match lists.get_mut(name) {
    Some(list) => list.push(position),
    None => {
        lists.insert(name.to_owned(), vec![position]);
    }
    }
}

impl CoreIndexes {
    // Indexes for entries that already form a valid history, such as a view's.
    pub(crate) fn build(entries: &[Entry]) -> Self {
        let mut indexes = Self::default();
        for entry in entries {
            indexes.push(entry);
        }
        indexes
    }

    #[cfg(test)]
    pub(crate) fn first_children(&self) -> Vec<Option<usize>> {
        self.children
            .iter()
            .map(|children| children.first().copied())
            .collect()
    }

    pub(crate) fn len(&self) -> usize {
        self.roots.len()
    }

    // Appends one entry that passed validation against the indexed entries
    // before it, so every reference names an earlier node.
    pub(crate) fn push(&mut self, entry: &Entry) {
        let position = self.len();
        self.children.push(Vec::new());
        self.incident.push(Vec::new());
        match entry {
        Entry::Node(node) => {
            let root = match node.predecessor {
            Some(predecessor) => {
                self.children[predecessor.position].push(position);
                self.roots[predecessor.position]
            }
            None => position,
            };
            self.roots.push(root);
            push_keyed(&mut self.nodes_by_type, &node.ty_name, position);
            self.node_content
                .entry(node_hash(&node.ty_name, node.predecessor, &node.payload))
                .or_default()
                .push(position);
            if node.ty_name == crate::typing::Type::TYPE_NAME {
                self.declarations.push(Declaration::of(node));
            }
        }
        Entry::Edge(edge) => {
            self.roots.push(position);
            self.incident[edge.from.position].push(position);
            self.incident[edge.to.position].push(position);
            push_keyed(&mut self.edges_by_label, &edge.ty_name, position);
            self.edge_triple
                .entry(edge_hash(edge.from, edge.to, &edge.ty_name))
                .or_default()
                .push(position);
        }
        }
    }
}

/// The core indexes as one view sees them (SPEC §9.1). Every query is
/// bounded by the view: positions at or past its boundary are never returned,
/// a key that occurs only there is indistinguishable from an absent one, and
/// entry-keyed values are read only through an identity the view holds.
/// Results equal those of the matching `View` query on a view without
/// indexes, in append order.
#[derive(Clone, Copy)]
pub struct Indexes<'a> {
    entries: &'a [Entry],
    core: &'a CoreIndexes,
}

// Prints the boundary only: index contents can name positions past it.
impl fmt::Debug for Indexes<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Indexes")
            .field("end_exclusive", &self.entries.len())
            .finish_non_exhaustive()
    }
}

impl<'a> Indexes<'a> {
    pub(crate) fn new(entries: &'a [Entry], core: &'a CoreIndexes) -> Self {
        debug_assert!(core.len() >= entries.len(), "indexes cover the view");
        Self { entries, core }
    }

    /// The boundary of the view these indexes answer for.
    pub fn end_exclusive(self) -> usize {
        self.entries.len()
    }

    // The part of an append-ordered list inside the view.
    fn visible(self, positions: &'a [usize]) -> &'a [usize] {
        &positions[..positions.partition_point(|&p| p < self.entries.len())]
    }

    // The slot of an exact identity of this view, if it has one.
    fn slot(self, id: EntryId) -> Option<usize> {
        self.entries
            .get(id.position)
            .filter(|entry| entry.id() == id)
            .map(|_| id.position)
    }

    fn nodes_at(
        self,
        positions: &'a [usize],
    ) -> impl DoubleEndedIterator<Item = &'a Node> + use<'a> {
        let entries = self.entries;
        positions
            .iter()
            .map(move |&p| entries[p].as_node().expect("indexed node"))
    }

    fn edges_at(
        self,
        positions: &'a [usize],
    ) -> impl DoubleEndedIterator<Item = &'a Edge> + use<'a> {
        let entries = self.entries;
        positions
            .iter()
            .map(move |&p| entries[p].as_edge().expect("indexed edge"))
    }

    fn list(self, lists: &'a [Vec<usize>], id: EntryId) -> &'a [usize] {
        self.slot(id)
            .map_or(&[][..], |position| self.visible(&lists[position]))
    }

    /// K1: the direct successors of `id` in the view.
    pub fn children(self, id: EntryId) -> impl DoubleEndedIterator<Item = &'a Node> + use<'a> {
        self.nodes_at(self.list(&self.core.children, id))
    }

    /// K1: whether a node of the view has a successor in it; `None` for an
    /// identity the view does not hold or an edge.
    pub fn has_child(self, id: EntryId) -> Option<bool> {
        let position = self.slot(id)?;
        self.entries[position].as_node()?;
        Some(
            self.core.children[position]
                .first()
                .is_some_and(|&child| child < self.entries.len()),
        )
    }

    /// K1: every node below `id` in its revision tree, excluding `id`, in
    /// append order.
    pub fn descendants(self, id: EntryId) -> impl DoubleEndedIterator<Item = &'a Node> + use<'a> {
        let mut found = Vec::new();
        let mut pending = self.list(&self.core.children, id).to_vec();
        while let Some(position) = pending.pop() {
            found.push(position);
            pending.extend_from_slice(self.visible(&self.core.children[position]));
        }
        found.sort_unstable();
        let entries = self.entries;
        found
            .into_iter()
            .map(move |p| entries[p].as_node().expect("indexed node"))
    }

    /// K2: the edges incident to `id` in the view, in either direction.
    pub fn edges_of(self, id: EntryId) -> impl DoubleEndedIterator<Item = &'a Edge> + use<'a> {
        self.edges_at(self.list(&self.core.incident, id))
    }

    /// K3: the nodes of the view whose type name is `ty_name`.
    pub fn nodes_of_type(
        self,
        ty_name: &str,
    ) -> impl DoubleEndedIterator<Item = &'a Node> + use<'a> {
        let positions = self
            .core
            .nodes_by_type
            .get(ty_name)
            .map_or(&[][..], |positions| self.visible(positions));
        self.nodes_at(positions)
    }

    /// K4: the edges of the view labelled `ty_name`.
    pub fn edges_labelled(
        self,
        ty_name: &str,
    ) -> impl DoubleEndedIterator<Item = &'a Edge> + use<'a> {
        let positions = self
            .core
            .edges_by_label
            .get(ty_name)
            .map_or(&[][..], |positions| self.visible(positions));
        self.edges_at(positions)
    }

    /// K5: the nodes of the view with this type name, predecessor, and
    /// payload, current or not.
    pub fn nodes_with_content<'q>(
        self,
        ty_name: &'q str,
        predecessor: Option<EntryId>,
        payload: &'q Body,
    ) -> impl DoubleEndedIterator<Item = &'a Node> + use<'a, 'q> {
        let positions = self
            .core
            .node_content
            .get(&node_hash(ty_name, predecessor, payload))
            .map_or(&[][..], |positions| self.visible(positions));
        // A content hash only narrows the search; complete values decide.
        self.nodes_at(positions).filter(move |node| {
            node.ty_name == ty_name && node.predecessor == predecessor && node.payload == *payload
        })
    }

    /// K6: the edge of the view from `from` to `to` labelled `ty_name`, of
    /// which validation admits at most one.
    pub fn edge_between(self, from: EntryId, to: EntryId, ty_name: &str) -> Option<&'a Edge> {
        let positions = self
            .core
            .edge_triple
            .get(&edge_hash(from, to, ty_name))
            .map_or(&[][..], |positions| self.visible(positions));
        self.edges_at(positions)
            .find(|edge| edge.from == from && edge.to == to && edge.ty_name == ty_name)
    }

    /// K7: the root of a node of the view, which is the node itself for a root.
    pub fn root_of(self, id: EntryId) -> Option<EntryId> {
        let position = self.slot(id)?;
        self.entries[position].as_node()?;
        // A root precedes its descendants, so it is inside the view.
        Some(self.entries[self.core.roots[position]].id())
    }

    /// K8: the parse of a `Type` node of the view.
    pub fn declaration(self, id: EntryId) -> Option<&'a Declaration> {
        let position = self.slot(id)?;
        let types = self
            .core
            .nodes_by_type
            .get(crate::typing::Type::TYPE_NAME)?;
        let index = types.binary_search(&position).ok()?;
        Some(&self.core.declarations[index])
    }

    /// K3 and K8: every `Type` node of the view with its parse, current or
    /// not.
    pub fn type_declarations(self) -> impl Iterator<Item = (&'a Node, &'a Declaration)> + use<'a> {
        let types = self
            .core
            .nodes_by_type
            .get(crate::typing::Type::TYPE_NAME)
            .map_or(&[][..], |positions| self.visible(positions));
        // K8 is parallel to the complete K3 list, of which `types` is a prefix.
        self.nodes_at(types).zip(&self.core.declarations)
    }
}

#[cfg(test)]
mod test;
