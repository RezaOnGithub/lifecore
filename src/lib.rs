use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt, hash::Hash};
use strum::{EnumIs, EnumTryAs};

/// The Index For Any Event
///
/// All LifeCore entries are ordered, and may build on top of each other.
///
/// *Note:* index must be less than `usize::MAX`.
#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
#[cfg_attr(test, derive(Debug))]
pub struct EntryIndex(pub usize);

/// Most methods in LifeCore need to resolve a context for an event.
/// This is primarily for the type and its constraints upon entry insertion
/// (which can shift after a node has been added) but has uses in other systems.
/// We consider `end` to be exclusive; i.e. the context is the range `0..end`
/// (`0` inclusive, `end` exclusive)
#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
#[cfg_attr(test, derive(Debug))]
pub struct EntryContext {
    thing: usize,
    end: usize,
}

/// A window of entries in the database.
///
/// For most operations, the database is scanned in progressively larger
/// windows. `0..end` may be past the bound of the database; which implies the
/// whole database.
///
/// LifeCore events are strictly ordered and append-only. Given that meanings
/// and intentions can shift over time, graph operations need a reference window
/// even when inspecting an individual event.
#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
#[cfg_attr(test, derive(Debug))]
pub struct Context {
    end: usize,
}

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

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Hash, Default)]
pub struct Body(pub BTreeMap<String, Payload>);

/// "Circumstance" is currently a simple method of logging entry dates and
/// places Not much weight to it at the moment, and doesn't make a functional
/// difference in usual operation.
///
/// "Mock" circumstance is intended for running validation and analysis before
/// writing nodes or edges to the database proper. `Mock` circumstances must be
/// at the tail of circumstance list, and cannot appear in the middle.
#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Hash)]
pub enum Circumstance {
    Mock,
    Session {
        when_1h: Zoned,
        hostname: String,
        username: String,
    },
}

/// Error type for this crate. Uses errors from `jiff` and `whoami`
#[derive(thiserror::Error, Debug)]
#[cfg_attr(test, derive(strum::EnumDiscriminants))]
#[cfg_attr(test, strum_discriminants(name(LifeCoreErrorTest)))]
#[cfg_attr(test, strum_discriminants(vis(pub(crate))))]
#[cfg_attr(
    test,
    strum_discriminants(derive(strum::EnumIter, strum::VariantArray))
)]
#[cfg_attr(
    test,
    strum_discriminants(
        doc = "Helper enum to make sure we are testing for every failure scenario at least once"
    )
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
    #[error("Systems Error")]
    SystemsError(Vec<analysis::Inconsistency>),
    #[error("Validation Failure")]
    BasicValidationFailure(BasicValidationFailure),
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct BasicValidationFailure {
    pub offending_event: DiagnosticLocation,
    pub reason: BasicValidationFailureScenario,
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
pub enum Payload {
    PreciseTime(Timestamp),
    Text(String),
    Number(i64),
    Aggregate(Vec<Payload>),

    /// Opaque. May or may not be stored in an optimized manner on disk
    Serialized(JSON),
}

/// Describes whether a field is mandatory or not.
///
/// A field can be:
/// - Mandatory; must be filled out
/// - Encouraged: should be filled out but could remain empty
/// - Deprecated: should not be filled out but could be filled out
/// - Optional: may or may not be filled out
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub enum FieldStatus {
    Mandatory,
    Encouraged,
    Deprecated,
    Optional,
}

/// Convenience type for a Node.
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
    pub ty_name: String,
    pub field: Body,
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

/// The return type for [`LifeCore::families`]
#[derive(Clone, EnumIs, EnumTryAs)]
pub enum LifeCoreFamily {
    Leaf(PointedEventContext),
    Parent(PointedEventContext, Vec<LifeCoreFamily>),
}

/// The primary trait describing the interface to the database.
///
/// Mutations are committed only after the resulting database passes basic
/// validation. Ordinary mutations therefore fail while a mock suffix is
/// pending, without changing the database.
pub trait LifeCore: Send + Sync {
    /// Get last event in LifeCore as a bounded EventContext
    fn last(&self) -> Option<PointedEventContext>;
    /// Total event count; no index for any event is larger than or equal to
    /// this number
    fn total_event_count(&self) -> usize;
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
    /// Add a provisional edge to the validated mock suffix.
    ///
    /// A validation failure leaves the database unchanged.
    fn connect_mock(
        &mut self,
        from: PointedEventContext,
        ty: &str,
        to: PointedEventContext,
    ) -> Result<EdgeExt, LifeCoreError>;
    fn add(
        &mut self,
        ty: &str,
        payload: Body,
        parent: Option<PointedEventContext>,
    ) -> Result<NodeExt, LifeCoreError>;
    /// Add a provisional node to the validated mock suffix.
    ///
    /// A validation failure leaves the database unchanged.
    fn add_mock(
        &mut self,
        ty: &str,
        payload: Body,
        parent: Option<PointedEventContext>,
    ) -> Result<NodeExt, LifeCoreError>;
    /// Whether this database contains a validated suffix of mock events.
    fn is_mock(&self) -> bool;
    /// Replace the mock suffix's circumstance with one session generated at
    /// application time.
    ///
    /// This is a no-op when no mock events are present. A failure leaves the
    /// database unchanged.
    fn apply(&mut self) -> Result<(), LifeCoreError>;
    /// Get the event selected by a pointed context.
    fn get(&self, e: PointedEventContext) -> Option<EventExt>;
    fn get_with_circumstance(&self, e: PointedEventContext) -> Option<(EventExt, Circumstance)>;
    fn window<I>(&self, i: I) -> impl Iterator<Item = EventExt>
    where
        I: IntoIterator<Item = PointedEventContext>;
    /// Check whether `e.thing` is stale within context. Return None for invalid
    /// input Stale Node means a newer node has it as its parent within
    /// context Stale Edge means at least one end is a Stale Node
    fn is_stale(&self, e: PointedEventContext) -> Option<bool>;
    fn node(&self, ctx: PointedEventContext) -> Option<NodeExt>;
    fn edge(&self, ctx: PointedEventContext) -> Option<EdgeExt>;
    fn nodes(&self, ctx: EventContext) -> impl Iterator<Item = NodeExt>;
    fn edges(&self, ctx: EventContext) -> impl Iterator<Item = EdgeExt>;
    fn fresh_nodes(&self, ctx: EventContext) -> impl Iterator<Item = NodeExt> {
        self.nodes(ctx).filter(|x| !self.is_stale(x.e).unwrap())
    }
    fn fresh_edges(&self, ctx: EventContext) -> impl Iterator<Item = EdgeExt> {
        self.edges(ctx).filter(|x| !self.is_stale(x.e).unwrap())
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
            !self.is_stale(n.look_at(x.e)).unwrap()
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
            .filter(move |x| !self.is_stale(x.e).unwrap() && x.from.absolute() == n.absolute())
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
            .filter(move |x| !self.is_stale(x.e).unwrap() && x.to.absolute() == n.absolute())
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
    fn families(&self, ctx: EventContext) -> Vec<LifeCoreFamily>;
    fn add_analyzer(&mut self, name: &str, config: &str);
    fn current_analysis_suite(&self) -> analysis::SuiteConfig;
}

/// Canonical implementor of LifeCore
pub use internal::LifeCoreList;

pub use typing::{NodeType, NodeTypeClass, NodeTypeFieldStatus};

/// A trait for downstream Analyzer and NodeType implementors
pub trait TypedBody<'a>: Into<Body> + TryFrom<&'a Body> {
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

/// The location associated with a diagnostic.
#[derive(PartialEq, Eq, Clone, Copy, Hash, Debug, PartialOrd, Ord)]
pub enum DiagnosticLocation {
    Window(EventContext),
    Event(PointedEventContext),
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

impl Body {
    pub fn from(x: &[(&str, Payload)]) -> Self {
        let mut r = BTreeMap::<String, Payload>::new();
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
}

impl Payload {
    pub fn discriminant(&self) -> PayloadType {
        match self {
        Payload::PreciseTime(_) => PayloadType::PreciseTime,
        Payload::Text(_) => PayloadType::Text,
        Payload::Number(_) => PayloadType::Number,
        Payload::Serialized(_) => PayloadType::Serialized,
        }
    }
}

/// Keyword Arguments to [`LifeCoreFamily::insert`]
#[derive(Clone, Copy)]
pub struct FamilyInsert {
    pub parent: PointedEventContext,
    pub child: PointedEventContext,
}

impl fmt::Display for Payload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
        Payload::PreciseTime(timestamp) => f.write_fmt(format_args!("{}", timestamp)),
        Payload::Text(s) => f.write_fmt(format_args!("\"{}\"", s.as_str())),
        Payload::Number(i) => f.write_fmt(format_args!("{}", i)),
        Payload::Serialized(s) => f.write_fmt(format_args!("<{}>", s)),
        }
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

pub mod analysis;
mod internal;
#[cfg(test)]
mod test;
mod typing;

mod boilerplate;
