use std::{ops::Index, ops::Range};

use crate::{
    BasicValidationFailure, Body, Context, DiagnosticLocation, Edge, Entry, EntryContext, EntryIndex, FamilyInsert, LifeCoreError, LifeCoreFamily, Node, Payload,
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

// TODO expand to other types
impl Payload {
    pub fn as_number(&self) -> i64 {
        self.clone().try_as_number().unwrap()
    }
}
