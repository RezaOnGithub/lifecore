use std::{
    ops::{Index, Range},
    str::FromStr,
};

use indexmap::IndexSet;
use itertools::Itertools;

use crate::{
    BasicValidationFailure, Body, Context, DiagnosticLocation, Edge, Entry, EntryContext,
    EntryIndex, FamilyInsert, FieldQuery, FieldQueryPathComponent, LifeCoreError, LifeCoreFamily,
    Node, NodeQueryCombinator, NodeQueryExpression, NodeQueryPredicate, OrderedSet, Payload,
};

mod indexing_and_context {
    use super::*;
    impl From<&usize> for EntryIndex {
        fn from(value: &usize) -> Self {
            assert!(*value < usize::MAX);
            Self(*value)
        }
    }

    impl From<usize> for EntryIndex {
        fn from(value: usize) -> Self {
            (&value).into()
        }
    }

    // NOTE these operations throw away information! Commented out for now
    // impl From<&EntryContext> for EntryIndex {
    //     fn from(value: &EntryContext) -> Self {
    //         Self(value.thing)
    //     }
    // }
    // impl From<EntryContext> for EntryIndex {
    //     fn from(value: EntryContext) -> Self {
    //         (&value).into()
    //     }
    // }

    // NOTE these operations make up information! Uncommented for now
    impl From<&EntryIndex> for EntryContext {
        fn from(value: &EntryIndex) -> Self {
            value.before_and_including()
        }
    }

    impl From<EntryIndex> for EntryContext {
        fn from(value: EntryIndex) -> Self {
            value.before_and_including()
        }
    }

    impl From<&EntryContext> for Context {
        fn from(value: &EntryContext) -> Self {
            Self { end: value.end }
        }
    }

    impl From<EntryContext> for Context {
        fn from(value: EntryContext) -> Self {
            (&value).into()
        }
    }

    impl EntryIndex {
        pub fn before(&self) -> Context {
            Context { end: self.0 }
        }

        pub fn before_and_including(&self) -> EntryContext {
            EntryContext {
                thing: self.0,
                end: self.0.saturating_add(1),
            }
        }

        pub fn absolute(&self) -> EntryContext {
            Context::universe().look_at(self.before_and_including())
        }
    }

    impl Context {
        pub(crate) fn range(&self) -> Range<usize> {
            (0..self.end).into()
        }

        pub fn last(&self) -> Option<EntryContext> {
            if self.range().is_empty() {
                None
            } else {
                Some(EntryContext {
                    thing: self.end.saturating_sub(1),
                    end: self.end,
                })
            }
        }

        /// *Note:* If `first` exists, it is always the first entry in the
        /// database.
        pub fn first(&self) -> Option<EntryContext> {
            if self.range().is_empty() {
                None
            } else {
                Some(EntryContext {
                    thing: 0,
                    end: self.end,
                })
            }
        }

        // Look at every entry within context, from the context
        // NOTE potentially out of bounds! We frequently use `usize::MAX` to denote the
        // full window of all entries.
        pub fn items(self) -> impl Iterator<Item = EntryContext> {
            let ctx = self;
            self.range().into_iter().map(move |x| EntryContext {
                thing: x,
                end: self.end,
            })
        }

        pub fn is_empty(&self) -> bool {
            self.range().is_empty()
        }

        /// NOTE throws away the context on `e` and looks at its event from
        /// `self`'s perspective.
        pub fn look_at(&self, e: EntryContext) -> EntryContext {
            let r = EntryContext {
                thing: e.thing,
                end: self.end,
            };
            assert!(r.validate());
            r
        }

        pub fn universe() -> Self {
            Self { end: usize::MAX }
        }
    }

    impl EntryContext {
        pub(crate) fn range(&self) -> Range<usize> {
            (0..self.end).into()
        }

        /// Throw away the context and get the index without it.
        ///
        /// *Note:* This method throws away information! Handle with care.
        pub fn entry(&self) -> EntryIndex {
            EntryIndex(self.thing)
        }

        pub fn validate(&self) -> bool {
            self.range().contains(&self.thing)
        }

        pub fn window(self) -> Context {
            Context { end: self.end }
        }

        pub fn absolute(self) -> Self {
            Context::universe().look_at(self)
        }

        pub fn last(&self) -> Option<Self> {
            self.window().last()
        }

        pub fn first(&self) -> Option<Self> {
            self.window().first()
        }

        pub fn items(self) -> impl Iterator<Item = Self> {
            self.window().items()
        }

        pub fn shrink_to_inclusive_left(&self) -> Context {
            assert!(self.validate() && self.thing < usize::MAX);
            Context {
                end: self.thing + 1,
            }
        }

        pub fn shrink_to_exclusive_left(&self) -> Context {
            assert!(self.validate());
            Context { end: self.thing }
        }

        pub fn inclusive_right_items(self) -> impl Iterator<Item = Self> {
            (self.thing..self.end).map(move |thing| Self { thing, ..self })
        }

        pub fn exclusive_right_items(self) -> impl Iterator<Item = Self> {
            (self.thing.saturating_add(1)..self.end).map(move |thing| Self { thing, ..self })
        }

        pub fn inclusive_left_items(self) -> impl Iterator<Item = Self> {
            (0..self.thing.saturating_add(1)).map(move |thing| Self { thing, ..self })
        }

        pub fn exclusive_left_items(self) -> impl Iterator<Item = Self> {
            (0..self.thing).map(move |thing| Self { thing, ..self })
        }

        pub fn is_empty(&self) -> bool {
            assert!(self.validate());
            self.range().is_empty()
        }

        pub fn look_at(&self, e: Self) -> Self {
            self.window().look_at(e)
        }
    }

    impl Ord for EntryContext {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            match self.end.cmp(&other.end) {
            std::cmp::Ordering::Less => std::cmp::Ordering::Less,
            std::cmp::Ordering::Greater => std::cmp::Ordering::Greater,
            std::cmp::Ordering::Equal => self.thing.cmp(&other.thing),
            }
        }
    }

    impl IntoIterator for Context {
        type Item = usize;

        type IntoIter = <std::ops::Range<usize> as IntoIterator>::IntoIter;

        fn into_iter(self) -> Self::IntoIter {
            self.range().into_iter()
        }
    }

    impl IntoIterator for EntryContext {
        type Item = usize;

        type IntoIter = <std::ops::Range<usize> as IntoIterator>::IntoIter;

        fn into_iter(self) -> Self::IntoIter {
            self.range().into_iter()
        }
    }
}

impl From<Context> for DiagnosticLocation {
    fn from(value: Context) -> Self {
        Self::Context(value)
    }
}

impl From<EntryContext> for DiagnosticLocation {
    fn from(value: EntryContext) -> Self {
        Self::Entry(value)
    }
}

impl Index<&str> for Body {
    type Output = Payload;

    fn index(&self, index: &str) -> &Self::Output {
        self.0.index(index)
    }
}

impl From<whoami::Error> for LifeCoreError {
    fn from(val: whoami::Error) -> LifeCoreError {
        LifeCoreError::HostNameOrUserNameError { inner: val.into() }
    }
}

impl From<jiff::Error> for LifeCoreError {
    fn from(inner: jiff::Error) -> Self {
        LifeCoreError::ErrorRoundingTime { inner }
    }
}

impl From<BasicValidationFailure> for LifeCoreError {
    fn from(value: BasicValidationFailure) -> Self {
        Self::BasicValidationFailure(value)
    }
}

impl Entry {
    pub fn entry_context(&self) -> EntryContext {
        match self {
        Entry::Node(node) => node.e,
        Entry::Edge(edge) => edge.e,
        }
    }

    pub fn as_node(&self) -> Node {
        self.clone().try_as_node().unwrap()
    }

    pub fn as_edge(&self) -> Edge {
        self.clone().try_as_edge().unwrap()
    }

    pub fn is_node_and<F: Fn(Node) -> bool>(&self, f: F) -> bool {
        self.clone().try_as_node().is_some_and(f)
    }

    pub fn is_edge_and<F: Fn(Edge) -> bool>(&self, f: F) -> bool {
        self.clone().try_as_edge().is_some_and(f)
    }
}

impl LifeCoreFamily {
    pub fn contains_absolute(&self, v: EntryContext) -> bool {
        match self {
        LifeCoreFamily::Leaf(pointed_event_context) => {
            v.absolute() == pointed_event_context.absolute()
        }
        LifeCoreFamily::Parent(pointed_event_context, items) => {
            v.absolute() == pointed_event_context.absolute()
                || items.iter().any(|x| x.contains_absolute(v))
        }
        }
    }

    /// *Note:* inserts `v` as is. Silently fails if `v.parent` is not present.
    pub fn insert(&mut self, v: FamilyInsert) {
        if self.contains_absolute(v.parent) {
            match self {
            LifeCoreFamily::Leaf(pointed_event_context) => {
                assert!(pointed_event_context.absolute() == v.parent.absolute());
                *self = LifeCoreFamily::Parent(v.parent, vec![LifeCoreFamily::Leaf(v.child)])
            }
            LifeCoreFamily::Parent(pointed_event_context, items) => {
                if pointed_event_context.absolute() == v.parent {
                    items.push(LifeCoreFamily::Leaf(v.child));
                } else {
                    items.iter_mut().for_each(|x| x.insert(v));
                }
            }
            }
        }
    }

    pub fn len(&self) -> usize {
        match self {
        LifeCoreFamily::Leaf(_) => 1,
        LifeCoreFamily::Parent(_, items) => 1 + items.iter().map(|x| x.len()).sum::<usize>(),
        }
    }

    /// *Note:* inserts `v` as is.
    pub fn new_with_root(v: EntryContext) -> Self {
        Self::Leaf(v)
    }

    pub fn collect(&self) -> Vec<EntryContext> {
        match self {
        LifeCoreFamily::Leaf(pointed_event_context) => vec![*pointed_event_context],
        LifeCoreFamily::Parent(pointed_event_context, items) => {
            let mut r = vec![*pointed_event_context];
            for v in items.iter().map(|x| x.collect()) {
                r.extend(v);
            }
            r
        }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = EntryContext> {
        self.collect().into_iter()
    }
}

impl NodeQueryCombinator {
    pub fn reduce(&self, a: bool, b: bool) -> bool {
        match self {
        NodeQueryCombinator::All => a && b,
        NodeQueryCombinator::Any => a || b,
        NodeQueryCombinator::None => (!a) && (!b),
        }
    }
}

impl Node {
    pub fn get(&self, field_path: FieldQuery) -> Vec<Payload> {
        let Some(payload) = self.field.0.get(field_path.field_name.as_str()) else {
            return vec![];
        };
        payload.get(&field_path.path)
    }
}

impl Payload {
    pub fn get(&self, path: &Vec<FieldQueryPathComponent>) -> Vec<Payload> {
        if path.is_empty() {
            vec![self.clone()]
        } else {
            if !self.is_sequence() || !self.is_ordered_set() {
                return vec![];
            }
            let lower_path = path[1..].iter().cloned().collect_vec();

            // Because "Set" items do have an ordering, we need to check whether `self` is a
            // set or a sequence
            let inner_items = {
                if self.is_sequence() {
                    self.clone().try_as_sequence().unwrap()
                } else {
                    self.clone()
                        .try_as_ordered_set()
                        .unwrap()
                        .0
                        .iter()
                        .cloned()
                        .collect_vec()
                }
            };
            match path[0] {
            FieldQueryPathComponent::Last => {
                inner_items.last().map(|x| x.get(&lower_path)).unwrap()
            }
            FieldQueryPathComponent::Wildcard => inner_items
                .iter()
                .map(|x| x.get(&lower_path))
                .flatten()
                .collect(),
            FieldQueryPathComponent::Index(index) => {
                inner_items.get(index).map(|x| x.get(&lower_path)).unwrap()
            }
            }
        }
    }

    // Let's not implement FromStr because... something about parsing? TODO
    pub fn text(s: &str) -> Self {
        Payload::Text(s.to_string())
    }

    pub fn sequence(x: &[Payload]) -> Payload {
        Payload::Sequence(x.iter().cloned().collect())
    }
}

macro_rules! payload_number_try_from {
    ($T:ty) => {
        impl TryFrom<$T> for Payload {
            type Error = LifeCoreError;

            fn try_from(value: $T) -> Result<Self, Self::Error> {
                let x = TryInto::<i64>::try_into(value);
                if x.is_err() {
                    return Err(LifeCoreError::Bap);
                }
                Ok(Payload::Number(x.unwrap()))
            }
        }
    };
}
payload_number_try_from!(usize);

impl From<i64> for Payload {
    fn from(value: i64) -> Self {
        Payload::Number(value)
    }
}

impl NodeQueryExpression {
    pub fn predicates(&self) -> &Vec<NodeQueryPredicate> {
        match self {
        NodeQueryExpression::All(node_query_predicates) => node_query_predicates,
        NodeQueryExpression::Any(node_query_predicates) => node_query_predicates,
        NodeQueryExpression::None(node_query_predicates) => node_query_predicates,
        }
    }
}

use std::hash::Hash;

impl<T: Eq + Hash> Hash for OrderedSet<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // This ensures that OrderedSet is always unique from Sequence
        const UUID: uuid::Uuid = uuid::uuid!("019f849d-0a29-798c-b717-c8c405d07b5a");
        for byte in UUID.as_bytes() {
            byte.hash(state);
        }
        for element in self.0.as_slice() {
            element.hash(state);
        }
    }
}

// TODO expand to other types
impl Payload {
    pub fn as_number(&self) -> i64 {
        self.clone().try_as_number().unwrap()
    }
}

impl<T: Hash + Eq> FromIterator<T> for OrderedSet<T> {
    fn from_iter<A: IntoIterator<Item = T>>(iter: A) -> Self {
        Self(IndexSet::from_iter(iter))
    }
}

impl<T: Eq + Hash> Default for OrderedSet<T> {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl<T: Eq + Hash> OrderedSet<T> {
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.0.iter()
    }

    pub fn contains(&self, value: &T) -> bool {
        self.0.contains(value)
    }
}
