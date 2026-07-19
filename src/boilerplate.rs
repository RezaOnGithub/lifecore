use std::{collections::BTreeMap, ops::Index, ops::Range};

use crate::{
    BasicValidationFailure, Body, Context, EntryContext, EntryIndex, LifeCoreError, LifeCoreFamily,
    Payload,
};

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

impl From<&EntryContext> for EntryIndex {
    fn from(value: &EntryContext) -> Self {
        Self(value.event)
    }
}

impl From<EntryContext> for EntryIndex {
    fn from(value: EntryContext) -> Self {
        (&value).into()
    }
}

impl EntryIndex {
    pub fn before(&self) -> Context {
        Context { end: self.i() }
    }

    pub fn before_and_including(&self) -> EntryContext {
        EntryContext {
            event: self.i(),
            end: self.i().saturating_add(1),
        }
    }

    pub fn absolute(&self) -> EntryContext {
        Context::universe().look_raw(self.i())
    }

    fn i(self) -> usize {
        self.0 as usize
    }
}

impl Index<&str> for Body {
    type Output = Payload;

    fn index(&self, index: &str) -> &Self::Output {
        self.0.index(index)
    }
}

impl Context {
    pub(crate) fn range(&self) -> Range<usize> {
        (0..self.end).into()
    }

    pub(crate) fn look_raw(&self, e: usize) -> EntryContext {
        let r = EntryContext {
            event: e,
            end: self.end,
        };
        assert!(r.validate());
        r
    }

    pub fn last(&self) -> Option<EntryContext> {
        if self.range().is_empty() {
            None
        } else {
            Some(self.look_raw(self.end.saturating_sub(1)))
        }
    }

    pub fn first(&self) -> Option<EntryContext> {
        if self.range().is_empty() {
            None
        } else {
            Some(self.look_raw(0))
        }
    }

    // Look at every event within context, from the context
    // NOTE potentially unbounded!
    pub fn items(self) -> impl Iterator<Item = EntryContext> {
        let ctx = self;
        self.range().into_iter().map(move |x| ctx.look_raw(x))
    }

    pub fn is_empty(&self) -> bool {
        self.range().is_empty()
    }

    // Builders
    // Take care to write an interface that minimizes usage mistakes

    /// NOTE throws away the context on `e` and looks at its event from `self`'s
    /// perspective.
    pub fn look_at(&self, e: EntryContext) -> EntryContext {
        let r = EntryContext {
            event: e.event,
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

    /// This is new functionality due to growing pains with regard to user code.
    /// Handle with care!
    pub fn new(event: usize) -> Self {
        Self {
            event,
            end: event.saturating_add(1),
        }
    }

    pub fn validate(&self) -> bool {
        self.range().contains(&self.thing)
    }

    pub fn window(self) -> EventContext {
        EventContext { end: self.end }
    }

    pub fn absolute(self) -> Self {
        EventContext::universe().look_at(self)
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

    pub fn shrink_to_inclusive_left(&self) -> EventContext {
        assert!(self.validate() && self.thing < usize::MAX);
        EventContext {
            end: self.thing + 1,
        }
    }

    pub fn shrink_to_exclusive_left(&self) -> EventContext {
        assert!(self.validate());
        EventContext { end: self.thing }
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

    pub(crate) fn look_raw(&self, e: usize) -> Self {
        self.window().look_raw(e)
    }
}

impl Ord for PointedEventContext {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.end.cmp(&other.end) {
        std::cmp::Ordering::Less => std::cmp::Ordering::Less,
        std::cmp::Ordering::Greater => std::cmp::Ordering::Greater,
        std::cmp::Ordering::Equal => self.thing.cmp(&other.thing),
        }
    }
}

impl PartialOrd for PointedEventContext {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl From<PointedEventContext> for EventContext {
    fn from(value: PointedEventContext) -> Self {
        value.window()
    }
}

impl LifeCoreFamily {
    pub fn contains_absolute(&self, v: PointedEventContext) -> bool {
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
    pub fn new_with_root(v: PointedEventContext) -> Self {
        Self::Leaf(v)
    }

    pub fn collect(&self) -> Vec<PointedEventContext> {
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

    pub fn iter(&self) -> impl Iterator<Item = PointedEventContext> {
        self.collect().into_iter()
    }
}
