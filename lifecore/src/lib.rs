use itertools::Itertools;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::HashMap,
    fmt,
    hash::Hash,
    ops::{Index, Range},
};
use strum::{EnumIs, EnumTryAs};

/// An instant in time. From `jiff`.
pub use jiff::Timestamp;

/// A timezone-aware instant in time. From `jiff`.
pub use jiff::Zoned;

/// Payload type for a complex object. From `serde_json`
///
/// # NOTE
///
/// Use of JSON is highly discouraged; stick to primitive types and
/// consider this an escape hatch just in case.
pub use serde_json::Value as JSON;
pub use serde_json::to_value as to_json_value;

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct NodeBodyExt(pub HashMap<String, PayloadExt>);

/// "Circumstance" is currently a simple method of logging event dates and
/// places Not much weight to it at the moment, and doesn't make a functional
/// difference.
///
/// "Mock" circumstance is intended for running validation and analysis before
/// writing a node or an edge to the database proper. Currently, a single `Mock`
/// is allowed and it must be associated with the last event
#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Hash)]
pub enum CircumstanceExt {
    Mock,
    Session {
        when_1h: Zoned,
        hostname: String,
        username: String,
    },
}

#[derive(thiserror::Error, Debug)]
#[cfg_attr(test, derive(strum::EnumDiscriminants))]
#[cfg_attr(test, strum_discriminants(name(LifeCoreErrorTest)))]
#[cfg_attr(test, strum_discriminants(vis(pub(crate))))]
#[cfg_attr(
    test,
    strum_discriminants(derive(strum::EnumIter, strum::VariantArray))
)]
pub enum LifeCoreError {
    #[error("Some Oddity With Rounding Time!")]
    ErrorRoundingTime { inner: jiff::Error },
    #[error("Couldn't Examine Environment for Circumstance")]
    HostNameOrUserNameError { inner: std::io::Error },
    #[error("Field {field_name:?} Couldn't Be Deserialized!")]
    FieldDeserializationFailed { field_name: String },
    #[error("Field {field_name:?} was not found!")]
    FieldNotFound { field_name: String },
    #[error("Field {field_name:?} is not the type we expected it to be!")]
    FieldContainsUnexpectedType { field_name: String },
    #[error("Potentially different analyzers under the same name {dupe_name:?}!")]
    DuplicateAnalyzer { dupe_name: String },
    #[error("Analyzer {name:?} not found!")]
    AnalyzerNotFound { name: String },
    // TODO is this used anywhere?
    #[error("Systems Error")]
    SystemsError(Vec<analysis::Inconsistency>),
    #[error("Validation Failure")]
    BasicValidationFailure(BasicValidationFailure),
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct BasicValidationFailure {
    offending_event: DiagnosticLocation,
    reason: BasicValidationFailureScenario,
}

#[derive(PartialEq, Eq, Clone, Debug)]
#[cfg_attr(test, derive(strum::EnumDiscriminants))]
#[cfg_attr(test, strum_discriminants(name(BasicValidationTest)))]
#[cfg_attr(test, strum_discriminants(vis(pub(crate))))]
#[cfg_attr(
    test,
    strum_discriminants(derive(strum::EnumIter, strum::VariantArray))
)]
pub enum BasicValidationFailureScenario {
    InternalError { message: String },
    ExtraneousMockCircumstance,
    DuplicateEdge,
    DuplicateNode,
    StaleRuleViolation,
    OutOfRangeParent,
    EdgeParent,
    OutOfRangeEdgeEnd,
    EdgeEndIsEdge,
}

/// Convenient Payload Type For Users
///
/// # Note
///
/// `PayloadExt::Serialized` should be the last resort for users. As time goes
/// on, the list of primitve types will be expaneded to further minimize its
/// use. Serialized LOBs are highly discouraged.
#[derive(
    Serialize,
    Deserialize,
    Clone,
    PartialEq,
    Eq,
    Hash,
    strum::EnumIs,
    strum::EnumTryAs,
    strum::EnumDiscriminants,
    strum::VariantNames,
    Debug,
)]
#[strum_discriminants(name(PayloadType))]
#[strum_discriminants(vis(pub))]
#[strum_discriminants(derive(Serialize, Deserialize, Hash, strum::Display))]
#[strum_discriminants(
    doc = "Denotes a type of payload. Automatically generated from `PayloadExt` variants"
)]
pub enum PayloadExt {
    PreciseTime(Timestamp),
    Text(String),
    Number(i64),
    Serialized(JSON),
}

impl PayloadExt {
    pub fn serialized_from<T: Serialize>(x: T) -> Self {
        Self::Serialized(to_json_value(x).unwrap())
    }
}

/// Convenience type describing a Node.
///
/// # Note
/// - `ty` may not resolve to the type that was intended when the node was first
///   created;
/// types can evolve over time and are context-dependent.
/// - `ty` may could potentially be unresolved as well. Use `ty_name` in
///   analyzers.
#[derive(Clone, PartialEq, Eq)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub struct NodeExt {
    pub e: PointedEventContext,
    pub ty: Option<NodeType>,
    pub ty_name: String,
    pub field: NodeBodyExt,
    pub parent: Option<PointedEventContext>,
}

/// Convenience type describing an Edge.
#[derive(Clone, PartialEq, Eq)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub struct EdgeExt {
    pub e: PointedEventContext,
    pub ty_name: String,
    pub from: PointedEventContext,
    pub to: PointedEventContext,
}

#[derive(EnumIs, EnumTryAs, Clone, PartialEq, Eq)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub enum EventExt {
    Node(NodeExt),
    Edge(EdgeExt),
}

/// The primary trait describing the interface to the database.
pub trait LifeCore: Send + Sync {
    /// Get last event in LifeCore as a bounded EventContext
    fn last(&self) -> Option<PointedEventContext>;
    /// Test for the basic rules of LifeCore.
    ///
    /// # Rules
    /// TODO
    fn basic_validation(&self) -> Result<(), BasicValidationFailure>;
    fn connect(
        &mut self,
        from: PointedEventContext,
        ty: &str,
        to: PointedEventContext,
    ) -> Result<EdgeExt, LifeCoreError>;
    fn add(
        &mut self,
        ty: &str,
        payload: NodeBodyExt,
        parent: Option<PointedEventContext>,
    ) -> Result<NodeExt, LifeCoreError>;
    /// Get the event selected by a pointed context.
    fn get(&self, e: PointedEventContext) -> Option<EventExt>;
    fn get_with_circumstance(&self, e: PointedEventContext) -> Option<(EventExt, CircumstanceExt)>;
    fn window<I>(&self, i: I) -> impl Iterator<Item = EventExt>
    where
        I: IntoIterator<Item = PointedEventContext>;
    /// Check whether `e.thing` is stale within context
    /// Stale Node means a newer node has it as its parent within context
    /// Stale Edge means at least one end is a Stale Node
    fn is_stale(&self, e: PointedEventContext) -> bool;
    fn node(&self, ctx: PointedEventContext) -> Option<NodeExt>;
    fn edge(&self, ctx: PointedEventContext) -> Option<EdgeExt>;
    fn nodes(&self, ctx: EventContext) -> impl Iterator<Item = NodeExt>;
    fn edges(&self, ctx: EventContext) -> impl Iterator<Item = EdgeExt>;
    fn fresh_nodes(&self, ctx: EventContext) -> impl Iterator<Item = NodeExt> {
        self.nodes(ctx).filter(|x| !self.is_stale(x.e))
    }
    fn fresh_edges(&self, ctx: EventContext) -> impl Iterator<Item = EdgeExt> {
        self.edges(ctx).filter(|x| !self.is_stale(x.e))
    }

    /// Get non-stale edges connected to node `n`
    ///
    /// *Note:* for this function to return anything meaningful, `n` itself
    /// needs to be fresh within the context that implies. That is, if `n` is
    /// stale, any edge connected to it is also stale.
    ///
    /// # Example
    /// ```rust
    /// # use lifecore::{LifeCoreList, LifeCore, NodeBodyExt, NodeExt, EdgeExt};
    /// let mut lc = LifeCoreList::default();
    /// let a0 = lc.add("A", NodeBodyExt::default(), None).unwrap();
    /// let b0 = lc.add("B", NodeBodyExt::default(), None).unwrap();
    /// let _ = lc.connect(a0.e, "X", b0.e).unwrap();
    /// let b1 = lc.add("B", NodeBodyExt::default(), Some(b0.e)).unwrap();
    /// let _ = lc.connect(a0.e, "X", b1.e).unwrap();
    /// assert_eq!(lc.fresh_edges_of(b0.e).count(), 0);
    /// ```
    fn fresh_edges_of(&self, n: PointedEventContext) -> impl Iterator<Item = EdgeExt> {
        self.edges(n.window()).filter(move |x| {
            !self.is_stale(n.look_at(x.e))
                && (x.from.absolute() == n.absolute() || x.to.absolute() == n.absolute())
        })
    }

    /// Get fresh outgoing edges from `n` within the implied context.
    ///
    /// Outgoing edge means an edge where `n` is `from`, also known as the head
    /// of the edge
    ///
    /// *Note:* for this function to return anything meaningful, `n` itself
    /// needs to be fresh within the context that implies. That is, if `n` is
    /// stale, any edge connected to it is also stale.
    fn fresh_outgoing_edges_of(&self, n: PointedEventContext) -> impl Iterator<Item = EdgeExt> {
        self.edges(n.window())
            .filter(move |x| !self.is_stale(x.e) && x.from.absolute() == n.absolute())
    }

    /// Get fresh Incoming edges from `n` within the implied context.
    ///
    /// Incoming edge means an edge where `n` is `to`, also known as the tail of
    /// the edge
    ///
    /// *Note:* for this function to return anything meaningful, `n` itself
    /// needs to be fresh within the context that implies. That is, if `n` is
    /// stale, any edge connected to it is also stale.
    fn fresh_incoming_edges_to(&self, n: PointedEventContext) -> impl Iterator<Item = EdgeExt> {
        self.edges(n.window())
            .filter(move |x| !self.is_stale(x.e) && x.to.absolute() == n.absolute())
    }

    /// Get root of node at `e`. That is, the ancestor node by the way of
    /// `parent` component
    fn root_of(&self, e: PointedEventContext) -> Option<PointedEventContext>;

    /// Returns fresh successors within context.
    /// A 'successor' is defined as any non-stale node within context that has
    /// `node` as its ancestor. An 'ancestor' of a node is any node within
    /// `.parent` chain of it. If a node has no parent, it is its own ancestor
    fn fresh_successors(&self, node: PointedEventContext) -> Vec<PointedEventContext>;

    /// Find relative degree. Siblings (nodes that share a parent) have degree
    /// 0. Note that the left sides of the contexts need to extend far back
    /// enough in order to find the shared parent.
    fn relative_degree(&self, a: PointedEventContext, b: PointedEventContext) -> Option<usize>;
    fn are_siblings(&self, a: PointedEventContext, b: PointedEventContext) -> bool {
        self.relative_degree(a, b) == Some(0)
    }
    /// Return all the fresh types in context
    fn types(
        &self,
        e: EventContext,
    ) -> impl Iterator<Item = (String, (PointedEventContext, NodeType))>;
    fn non_solo_families(&self, ctx: EventContext) -> Vec<Vec<PointedEventContext>>;
    fn add_analyzer(&mut self, name: &str, config: &str);
    fn current_analysis_suite(&self) -> analysis::SuiteConfig;
}

/// Canonical implementor of LifeCore
pub use internal::LifeCoreList;

pub use typing::{NodeType, NodeTypeClass, NodeTypeFieldStatus};

/// A trait for downstream Analyzer and NodeType implementors
pub trait TypedBody<'a>: Into<NodeBodyExt> + TryFrom<&'a NodeBodyExt> {
    fn describe_self() -> NodeType;
    fn name() -> &'static str;
}

impl EventExt {
    pub fn as_node(&self) -> NodeExt {
        self.clone().try_as_node().unwrap()
    }

    pub fn as_edge(&self) -> EdgeExt {
        self.clone().try_as_edge().unwrap()
    }

    pub fn is_node_and<F: Fn(NodeExt) -> bool>(&self, f: F) -> bool {
        self.clone().try_as_node().is_some_and(f)
    }

    pub fn is_edge_and<F: Fn(EdgeExt) -> bool>(&self, f: F) -> bool {
        self.clone().try_as_edge().is_some_and(f)
    }

    pub fn ctx(&self) -> PointedEventContext {
        match self {
        EventExt::Node(node_ext) => node_ext.e,
        EventExt::Edge(edge_ext) => edge_ext.e,
        }
    }
}
/// A window from the beginning of graph history to an exclusive end.
///
/// LifeCore events are strictly ordered and append-only. Given that meanings
/// and intentions can shift over time, graph operations need a reference window
/// even when inspecting an individual event.
#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Copy, Hash, Debug, PartialOrd, Ord)]
pub struct EventContext {
    // Exclusive! `0..end` basically
    // Used to calculate type, check for staleness etc
    pub(crate) end: usize,
}

/// An [`EventContext`] window coupled with a concrete event in that window.
// TOTEST PointedEventContext can never point to usize::MAX
#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Copy, Hash, Debug, PartialOrd, Ord)]
pub struct PointedEventContext {
    pub(crate) end: usize,
    pub(crate) thing: usize,
}

/// The location associated with a diagnostic.
#[derive(PartialEq, Eq, Clone, Copy, Hash, Debug, PartialOrd, Ord)]
pub enum DiagnosticLocation {
    Window(EventContext),
    Event(PointedEventContext),
}

impl EventContext {
    pub(crate) fn range(&self) -> Range<usize> {
        0..self.end
    }

    pub fn last(&self) -> Option<PointedEventContext> {
        if self.range().is_empty() {
            None
        } else {
            Some(self.look_raw(self.end.saturating_sub(1)))
        }
    }

    pub fn first(&self) -> Option<PointedEventContext> {
        if self.range().is_empty() {
            None
        } else {
            Some(self.look_raw(0))
        }
    }

    // Look at every event within context, from the context
    // NOTE potentially unbounded!
    pub fn items(self) -> impl Iterator<Item = PointedEventContext> {
        let ctx = self;
        self.range().map(move |x| ctx.look_raw(x))
    }

    pub fn is_empty(&self) -> bool {
        self.range().is_empty()
    }

    // Builders
    // Take care to write an interface that minimizes usage mistakes

    /// NOTE throws away the context on `e` and looks at its event from `self`'s
    /// perspective.
    pub fn look_at(&self, e: PointedEventContext) -> PointedEventContext {
        let r = PointedEventContext {
            end: self.end,
            thing: e.thing,
        };
        assert!(r.validate());
        r
    }

    // NOTDO this is the only function that somewhat leaks implementation details.
    // Handle with care!
    pub(crate) fn look_raw(&self, e: usize) -> PointedEventContext {
        let r = PointedEventContext {
            end: self.end,
            thing: e,
        };
        assert!(r.validate());
        r
    }

    pub fn universe() -> Self {
        Self { end: usize::MAX }
    }
}

impl PointedEventContext {
    pub(crate) fn range(&self) -> Range<usize> {
        0..self.end
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

impl From<PointedEventContext> for EventContext {
    fn from(value: PointedEventContext) -> Self {
        value.window()
    }
}

impl From<EventContext> for DiagnosticLocation {
    fn from(value: EventContext) -> Self {
        Self::Window(value)
    }
}

impl From<PointedEventContext> for DiagnosticLocation {
    fn from(value: PointedEventContext) -> Self {
        Self::Event(value)
    }
}

impl IntoIterator for EventContext {
    type Item = usize;

    type IntoIter = <std::ops::Range<usize> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.range().into_iter()
    }
}

impl IntoIterator for PointedEventContext {
    type Item = usize;

    type IntoIter = <std::ops::Range<usize> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.range().into_iter()
    }
}

impl NodeBodyExt {
    pub fn from(x: &[(&str, PayloadExt)]) -> Self {
        let mut r = HashMap::<String, PayloadExt>::new();
        assert!(
            x.iter().map(|y| &y.0).counts().len() == x.len(),
            "Supplied list has a repeated key (not a proper set!)"
        );
        for (k, v) in x.iter() {
            r.insert(k.to_string(), v.clone());
        }
        Self(r)
    }

    // NOTE one helper method per Payload type
    pub fn get_precise_time(&self, name: &str) -> Result<Timestamp, LifeCoreError> {
        if !self.0.contains_key(name) {
            Err(LifeCoreError::FieldNotFound {
                field_name: name.to_string(),
            })
        } else if !self.0[name].is_precise_time() {
            Err(LifeCoreError::FieldContainsUnexpectedType {
                field_name: name.to_string(),
            })
        } else {
            Ok(self.0[name].clone().try_as_precise_time().unwrap())
        }
    }
    pub fn get_number(&self, name: &str) -> Result<i64, LifeCoreError> {
        if !self.0.contains_key(name) {
            Err(LifeCoreError::FieldNotFound {
                field_name: name.to_string(),
            })
        } else if !self.0[name].is_number() {
            Err(LifeCoreError::FieldContainsUnexpectedType {
                field_name: name.to_string(),
            })
        } else {
            Ok(self.0[name].clone().try_as_number().unwrap())
        }
    }
    pub fn get_text(&self, name: &str) -> Result<String, LifeCoreError> {
        if !self.0.contains_key(name) {
            Err(LifeCoreError::FieldNotFound {
                field_name: name.to_string(),
            })
        } else if !self.0[name].is_text() {
            Err(LifeCoreError::FieldContainsUnexpectedType {
                field_name: name.to_string(),
            })
        } else {
            Ok(self.0[name].clone().try_as_text().unwrap())
        }
    }

    pub fn get_deserialized<T: DeserializeOwned>(&self, name: &str) -> Result<T, LifeCoreError> {
        if !self.0.contains_key(name) {
            Err(LifeCoreError::FieldNotFound {
                field_name: name.to_string(),
            })
        } else if !self.0[name].is_serialized() {
            Err(LifeCoreError::FieldContainsUnexpectedType {
                field_name: name.to_string(),
            })
        } else {
            if let Result::Ok(x) =
                serde_json::from_value::<T>(self.0[name].clone().try_as_serialized().unwrap())
            {
                Ok(x)
            } else {
                Err(LifeCoreError::FieldDeserializationFailed {
                    field_name: name.to_string(),
                })
            }
        }
    }
}

impl PayloadExt {
    pub fn discriminant(&self) -> PayloadType {
        match self {
        PayloadExt::PreciseTime(_) => PayloadType::PreciseTime,
        PayloadExt::Text(_) => PayloadType::Text,
        PayloadExt::Number(_) => PayloadType::Number,
        PayloadExt::Serialized(_) => PayloadType::Serialized,
        }
    }
}

impl fmt::Display for PayloadExt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
        PayloadExt::PreciseTime(timestamp) => f.write_fmt(format_args!("{}", timestamp)),
        PayloadExt::Text(s) => f.write_fmt(format_args!("\"{}\"", s.as_str())),
        PayloadExt::Number(i) => f.write_fmt(format_args!("{}", i)),
        PayloadExt::Serialized(s) => f.write_fmt(format_args!("<{}>", s)),
        }
    }
}

impl Index<&str> for NodeBodyExt {
    type Output = PayloadExt;

    fn index(&self, index: &str) -> &Self::Output {
        self.0.index(index)
    }
}

impl fmt::Display for EventContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("EventContext[{}, {})", 0, self.end))
    }
}

impl fmt::Display for PointedEventContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "PointedEventContext[{}, <{}>, {})",
            0, self.thing, self.end
        ))
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

impl Default for NodeBodyExt {
    fn default() -> Self {
        Self(HashMap::new())
    }
}

pub mod analysis;
mod internal;
#[cfg(test)]
mod test;
mod typing;
