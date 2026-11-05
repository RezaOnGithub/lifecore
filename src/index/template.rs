// Index templates (SPEC §9.1): parameterized indexes from a fixed catalogue
// that analyzers request and the runner builds per run. Each template ships
// its reference scan, `TemplateScan`, which is the executable form of its
// definition; the property tests hold `TemplateIndex` to it at every
// boundary.

use super::CoreIndexes;
use crate::{Edge, Entry, EntryId, View, view::Either};
use std::{
    collections::{BTreeSet, HashMap},
    fmt,
    ops::Range,
};

/// An index shape of the catalogue of SPEC §9.1, requested through
/// `Analyzer::index_templates`. Templates are data: two analyzers that request
/// equal templates share one index.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IndexTemplate {
    /// The edges labelled `label` at the node at `end`, whose other endpoint's
    /// type name is in `other` when given.
    EdgesAt {
        label: String,
        end: End,
        other: Option<BTreeSet<String>>,
    },
    /// The same edges at every node of the lineage of the node at `end`.
    EdgesAtLineage {
        label: String,
        end: End,
        other: Option<BTreeSet<String>>,
    },
}

/// The endpoint of an edge that an [`IndexTemplate`] keys by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum End {
    From,
    To,
}

impl End {
    fn of(self, edge: &Edge) -> EntryId {
        match self {
        Self::From => edge.from,
        Self::To => edge.to,
        }
    }
    fn opposite(self) -> Self {
        match self {
        Self::From => Self::To,
        Self::To => Self::From,
        }
    }
}

impl IndexTemplate {
    // The parameters both templates share, and whether the key is a lineage.
    fn parts(&self) -> (&str, End, Option<&BTreeSet<String>>, bool) {
        match self {
        Self::EdgesAt { label, end, other } => (label, *end, other.as_ref(), false),
        Self::EdgesAtLineage { label, end, other } => (label, *end, other.as_ref(), true),
        }
    }

    // Whether the build lists `edge` under some key, given the type name of
    // its other endpoint: the node's own `ty_name`, fixed at append, not the
    // type it resolves to in a view. `TemplateScan` states the same filter
    // on its own, so that a mistake here cannot hide in the reference too.
    fn lists(&self, edge: &Edge, other_ty_name: &str) -> bool {
        let (label, _, other, _) = self.parts();
        edge.ty_name == label && other.is_none_or(|other| other.contains(other_ty_name))
    }

    /// The reference scan of this template over `view` (SPEC §9.1): the same
    /// queries as [`TemplateIndex`], computed from `view`'s own queries, so it
    /// is an analyzer's fallback when `View::template` gives `None`.
    /// To read the template when the view carries it and scan otherwise, use
    /// `View::template_or_scan`.
    pub fn scan<'a, 't>(&'t self, view: View<'a>) -> TemplateScan<'a, 't> {
        TemplateScan {
            view,
            template: self,
        }
    }
}

// The built templates of one run, over the entries of its head view. Each
// key is a position: the node at the template's end for `EdgesAt`, its root
// (K7) for `EdgesAtLineage`. Positions are compared exactly, so no hash can
// merge two keys. Like the core indexes, every list is in append order and
// a view narrows it by its length; nothing here is serialized or printed.
pub(crate) struct TemplateIndexes {
    len: usize,
    // Sorted by template and without duplicates, so that equal requests share
    // one index and a lookup is a binary search.
    pub(super) built: Vec<(IndexTemplate, HashMap<usize, Vec<usize>>)>,
}

impl TemplateIndexes {
    // One pass over `entries`, a valid history covered by `core`. Each edge
    // costs one hash lookup by label plus one push per template that lists it.
    pub(crate) fn build(
        entries: &[Entry],
        core: &CoreIndexes,
        templates: impl IntoIterator<Item = IndexTemplate>,
    ) -> Self {
        debug_assert!(core.len() >= entries.len(), "core indexes cover the view");
        let mut templates = templates.into_iter().collect::<Vec<_>>();
        templates.sort_unstable();
        templates.dedup();
        let mut by_label = HashMap::<&str, Vec<usize>>::new();
        for (index, template) in templates.iter().enumerate() {
            by_label.entry(template.parts().0).or_default().push(index);
        }
        let mut lists = vec![HashMap::<usize, Vec<usize>>::new(); templates.len()];
        for (position, entry) in entries.iter().enumerate() {
            let Entry::Edge(edge) = entry else {
                continue;
            };
            let Some(indices) = by_label.get(edge.ty_name.as_str()) else {
                continue;
            };
            for &index in indices {
                let template = &templates[index];
                let (_, end, _, lineage) = template.parts();
                // Endpoints of a valid history are earlier nodes.
                let other = entries[end.opposite().of(edge).position]
                    .as_node()
                    .expect("an edge's endpoint is a node");
                if !template.lists(edge, &other.ty_name) {
                    continue;
                }
                let at = end.of(edge).position;
                let key = if lineage { core.roots[at] } else { at };
                lists[index].entry(key).or_default().push(position);
            }
        }
        Self {
            len: entries.len(),
            built: templates.into_iter().zip(lists).collect(),
        }
    }

    fn find(&self, template: &IndexTemplate) -> Option<usize> {
        self.built
            .binary_search_by(|(built, _)| built.cmp(template))
            .ok()
    }

    // The built templates one analyzer requested, sorted, for its
    // `Templates`. Equal requests of several analyzers name one built index.
    pub(crate) fn grant(&self, requested: &[IndexTemplate]) -> Vec<usize> {
        let mut granted = requested
            .iter()
            .filter_map(|template| self.find(template))
            .collect::<Vec<_>>();
        granted.sort_unstable();
        granted.dedup();
        granted
    }
}

// The templates one analyzer's views answer for: the run's built templates,
// narrowed to those it requested, so that what it reads never depends on
// what another analyzer of the suite requested.
#[derive(Clone, Copy)]
pub(crate) struct Templates<'a> {
    built: &'a TemplateIndexes,
    granted: &'a [usize],
}

impl<'a> Templates<'a> {
    pub(crate) fn new(built: &'a TemplateIndexes, granted: &'a [usize]) -> Self {
        Self { built, granted }
    }

    pub(crate) fn len(self) -> usize {
        self.built.len
    }

    fn get(
        self,
        template: &IndexTemplate,
    ) -> Option<&'a (IndexTemplate, HashMap<usize, Vec<usize>>)> {
        let index = self.built.find(template)?;
        self.granted.binary_search(&index).ok()?;
        Some(&self.built.built[index])
    }
}

/// A built index template as one view sees it (SPEC §9.1). Every query is
/// bounded by the view and answers in append order with the edges its
/// [`TemplateScan`] gives, so a key that occurs only past the boundary is
/// indistinguishable from an absent one.
#[derive(Clone, Copy)]
pub struct TemplateIndex<'a> {
    entries: &'a [Entry],
    core: &'a CoreIndexes,
    template: &'a IndexTemplate,
    lists: &'a HashMap<usize, Vec<usize>>,
}

// Prints the template and the boundary only: the lists can name positions
// past it.
impl fmt::Debug for TemplateIndex<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TemplateIndex")
            .field("template", self.template)
            .field("end_exclusive", &self.entries.len())
            .finish_non_exhaustive()
    }
}

impl<'a> TemplateIndex<'a> {
    pub(crate) fn new(
        entries: &'a [Entry],
        core: &'a CoreIndexes,
        templates: Templates<'a>,
        template: &IndexTemplate,
    ) -> Option<Self> {
        debug_assert!(templates.len() >= entries.len(), "templates cover the view");
        let (template, lists) = templates.get(template)?;
        Some(Self {
            entries,
            core,
            template,
            lists,
        })
    }

    /// The template this index was built for.
    pub fn template(self) -> &'a IndexTemplate {
        self.template
    }

    /// The boundary of the view this index answers for.
    pub fn end_exclusive(self) -> usize {
        self.entries.len()
    }

    // The listed positions of `key` inside the view: empty unless `key` is a
    // node the view holds exactly.
    fn visible(self, key: EntryId) -> &'a [usize] {
        let Some(position) = self
            .entries
            .get(key.position)
            .filter(|entry| entry.id() == key && entry.as_node().is_some())
            .map(|_| key.position)
        else {
            return &[];
        };
        let (_, _, _, lineage) = self.template.parts();
        // A root precedes its descendants, so it is inside the view too.
        let key = if lineage {
            self.core.roots[position]
        } else {
            position
        };
        self.lists.get(&key).map_or(&[][..], |positions| {
            &positions[..positions.partition_point(|&p| p < self.entries.len())]
        })
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

    // Whether a node of the view has a successor in it, by K1.
    fn is_stale_node(self, id: EntryId) -> bool {
        self.core.children[id.position]
            .first()
            .is_some_and(|&child| child < self.entries.len())
    }

    /// Returns the edges the template lists under `key` in the view, in
    /// append order.
    pub fn positions(self, key: EntryId) -> impl DoubleEndedIterator<Item = &'a Edge> + use<'a> {
        self.edges_at(self.visible(key))
    }

    /// Returns how many edges the template lists under `key` in the view.
    pub fn count(self, key: EntryId) -> usize {
        self.visible(key).len()
    }

    /// Returns the edges the template lists under `key` whose positions are
    /// in `range`, which may reach past the view, in append order.
    pub fn in_range(
        self,
        key: EntryId,
        range: Range<usize>,
    ) -> impl DoubleEndedIterator<Item = &'a Edge> + use<'a> {
        let positions = self.visible(key);
        let start = positions.partition_point(|&p| p < range.start);
        let end = positions.partition_point(|&p| p < range.end).max(start);
        self.edges_at(&positions[start..end])
    }

    /// Returns the edges the template lists under `key` that are current in
    /// the view, where neither endpoint has a successor, in append order.
    pub fn current(self, key: EntryId) -> impl DoubleEndedIterator<Item = &'a Edge> + use<'a> {
        self.positions(key)
            .filter(move |edge| !self.is_stale_node(edge.from) && !self.is_stale_node(edge.to))
    }
}

/// The reference scan of an [`IndexTemplate`] over a view (SPEC §9.1), with
/// the queries of [`TemplateIndex`] and the same results. It reads `View`'s
/// own queries, which use the core indexes when the view carries them.
#[derive(Clone, Copy, Debug)]
pub struct TemplateScan<'a, 't> {
    view: View<'a>,
    template: &'t IndexTemplate,
}

impl<'a, 't> TemplateScan<'a, 't> {
    /// Returns the edges the template lists under `key` in the view, in
    /// append order.
    pub fn positions(
        self,
        key: EntryId,
    ) -> impl DoubleEndedIterator<Item = &'a Edge> + use<'a, 't> {
        let Self { view, template } = self;
        // The definition of SPEC §9.1, stated without the build's helpers.
        let (label, end, other, lineage) = match template {
        IndexTemplate::EdgesAt { label, end, other } => (label.as_str(), *end, other, false),
        IndexTemplate::EdgesAtLineage { label, end, other } => (label.as_str(), *end, other, true),
        };
        // A key the view does not hold as a node lists nothing; a lineage is
        // named by any of its nodes.
        let root = view.node(key).and_then(|_| {
            if lineage {
                view.root_of(key)
            } else {
                Some(key)
            }
        });
        let candidates = match root {
        None => Vec::new(),
        Some(_) if lineage => view.edges_labelled(label).collect(),
        Some(_) => view.edges_of(key).collect(),
        };
        candidates.into_iter().filter(move |edge| {
            let (at, far) = match end {
            End::From => (edge.from, edge.to),
            End::To => (edge.to, edge.from),
            };
            let at_key = if lineage {
                view.root_of(at) == root
            } else {
                Some(at) == root
            };
            let far = view
                .node(far)
                .expect("an edge's endpoint is a node of its view");
            edge.ty_name == label
                && at_key
                && match other {
                None => true,
                Some(names) => names.contains(&far.ty_name),
                }
        })
    }

    /// Returns how many edges the template lists under `key` in the view.
    pub fn count(self, key: EntryId) -> usize {
        self.positions(key).count()
    }

    /// Returns the edges the template lists under `key` whose positions are
    /// in `range`, in append order.
    pub fn in_range(
        self,
        key: EntryId,
        range: Range<usize>,
    ) -> impl DoubleEndedIterator<Item = &'a Edge> + use<'a, 't> {
        self.positions(key)
            .filter(move |edge| range.contains(&edge.id.position))
    }

    /// Returns the edges the template lists under `key` that are current in
    /// the view, in append order.
    pub fn current(self, key: EntryId) -> impl DoubleEndedIterator<Item = &'a Edge> + use<'a, 't> {
        let view = self.view;
        self.positions(key)
            .filter(move |edge| view.is_stale(edge.id) == Some(false))
    }
}

/// A template's queries answered from its index when the view carries it and
/// by its reference scan otherwise, with the same results: the one-call
/// fallback of SPEC §9.1, from `View::template_or_scan`.
#[derive(Clone, Copy, Debug)]
pub struct TemplateQuery<'a, 't>(Source<'a, 't>);

#[derive(Clone, Copy, Debug)]
enum Source<'a, 't> {
    Index(TemplateIndex<'a>),
    Scan(TemplateScan<'a, 't>),
}

impl<'a, 't> TemplateQuery<'a, 't> {
    pub(crate) fn new(view: View<'a>, template: &'t IndexTemplate) -> Self {
        Self(match view.template(template) {
        Some(index) => Source::Index(index),
        None => Source::Scan(template.scan(view)),
        })
    }

    /// Returns the edges the template lists under `key` in the view, in
    /// append order.
    pub fn positions(
        self,
        key: EntryId,
    ) -> impl DoubleEndedIterator<Item = &'a Edge> + use<'a, 't> {
        match self.0 {
        Source::Index(index) => Either::Left(index.positions(key)),
        Source::Scan(scan) => Either::Right(scan.positions(key)),
        }
    }

    /// Returns how many edges the template lists under `key` in the view.
    pub fn count(self, key: EntryId) -> usize {
        match self.0 {
        Source::Index(index) => index.count(key),
        Source::Scan(scan) => scan.count(key),
        }
    }

    /// Returns the edges the template lists under `key` whose positions are
    /// in `range`, in append order.
    pub fn in_range(
        self,
        key: EntryId,
        range: Range<usize>,
    ) -> impl DoubleEndedIterator<Item = &'a Edge> + use<'a, 't> {
        match self.0 {
        Source::Index(index) => Either::Left(index.in_range(key, range)),
        Source::Scan(scan) => Either::Right(scan.in_range(key, range)),
        }
    }

    /// Returns the edges the template lists under `key` that are current in
    /// the view, in append order.
    pub fn current(self, key: EntryId) -> impl DoubleEndedIterator<Item = &'a Edge> + use<'a, 't> {
        match self.0 {
        Source::Index(index) => Either::Left(index.current(key)),
        Source::Scan(scan) => Either::Right(scan.current(key)),
        }
    }
}
