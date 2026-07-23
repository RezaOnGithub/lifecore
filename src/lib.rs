use jiff::Unit;
use serde::{Deserialize, Serialize};
use strum::Display;
use std::{collections::BTreeMap, hash::Hash};
use strum::{EnumIs, EnumTryAs};

/// The Index For Any Event
///
/// All LifeCore entries are ordered, and may build on top of each other.
///
/// *Note:* `usize::MAX` is not a valid index.
///
/// *Note:* In the past, we tried really hard to hide the actual number
/// representing the index. It is public now because someone realized they could
/// dig it out from the serialized value.
#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub struct EntryIndex(pub usize);

/// Most methods in LifeCore need to resolve a context for an event.
/// This is primarily for the type and its constraints upon entry insertion
/// (which can shift after a node has been added) but has uses in other systems.
/// We consider `end` to be exclusive; i.e. the context is the range `0..end`
/// (`0` inclusive, `end` exclusive)
///
/// *Note:* `Ord` is implemented manually to make sure `end` takes precedence
/// when sorting.
#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Clone, Copy, Hash, Debug)]
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
#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Debug, Clone, Copy, Hash)]
pub struct Context {
    end: usize,
}

/// The location associated with a diagnostic.
/// A diagnostic is either over a window of entries OR a specific entry
/// But diagnosing an issue associated with a specific entry most likely
/// requires the context for the it.
///
/// *Note:* Please be as specific as possible when creating values of this type.
/// We do filtering based on this value. This is not required for
/// [`analysis::Inconsistency`] and [`BasicValidationFailure`] as they must be
/// resolved immediately anyway.
#[derive(PartialEq, Eq, Clone, Copy, Hash, PartialOrd, Ord, Debug)]
pub enum DiagnosticLocation {
    Context(Context),
    Entry(EntryContext),
}

/// An instant in time. From `jiff`.
pub use jiff::Timestamp;

/// A timezone-aware instant in time. From `jiff`.
pub use jiff::Zoned;

/// A set of other Payload values with ordered elements. Based on
/// [`indexmap::IndexSet`]
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub struct OrderedSet<T: Eq + Hash>(indexmap::IndexSet<T>);

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Default, Hash)]
#[cfg_attr(debug_assertions, derive(Debug))]
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

impl Circumstance {
    fn current_session() -> Result<Self, LifeCoreError> {
        let now = Zoned::now().round(Unit::Hour)?;
        let hostname = whoami::hostname()?;
        let username = whoami::username()?;
        Ok(Self::Session {
            when_1h: now,
            hostname,
            username,
        })
    }
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
    #[error("Unable to parse FieldQuery")]
    FieldQueryParsingError,
    // TODO clean up this enum; removed funcionality
    // TODO comeup with better error string; caller that got this value is responsible for adding
    // more details.
    #[error("Unable to convert field payload to type")]
    NodeFieldDeserializationError {
        target_type: Option<String>,
        field_name: Option<String>,
    },
    // TODO create new, better errors for whatever uses this; remove it
    #[error("Bap!")]
    Bap,
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

// NOTE it is highly desirable for this to be `Eq` and `Hash` (namely, floating-point types would
// not be `Eq`)

// TODO add more time-related functionality
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
)]
#[cfg_attr(debug_assertions, derive(Debug))]
#[strum_discriminants(name(PayloadKind))]
#[strum_discriminants(vis(pub))]
#[strum_discriminants(derive(Serialize, Deserialize, Hash, Display))]
#[strum_discriminants(
    doc = "An enum with unit variants denoting every kind of Payload. Useful for queries."
)]
pub enum Payload {
    PreciseTime(Timestamp),
    Text(String),
    Number(i64),
    Sequence(Vec<Payload>),
    OrderedSet(OrderedSet<Payload>),
}

/// Convenience type for a Node.
#[derive(Clone, PartialEq, Eq)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub struct Node {
    pub e: EntryContext,
    pub ty_name: String,
    pub field: Body,
    pub parent: Option<EntryIndex>,
}

/// Convenience type describing an Edge.
#[derive(Clone, PartialEq, Eq)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub struct Edge {
    pub e: EntryContext,
    pub ty_name: String,
    pub from: EntryIndex,
    pub to: EntryIndex,
}

#[derive(EnumIs, EnumTryAs, Clone, PartialEq, Eq)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub enum Entry {
    Node(Node),
    Edge(Edge),
}

/// Keyword Arguments to [`LifeCoreFamily::insert`]
#[derive(Clone, Copy)]
pub struct FamilyInsert {
    pub parent: EntryContext,
    pub child: EntryContext,
}

/// The return type for [`LifeCore::families`]
#[derive(Clone, EnumIs, EnumTryAs)]
pub enum LifeCoreFamily {
    Leaf(EntryContext),
    Parent(EntryContext, Vec<LifeCoreFamily>),
}

pub struct NodeQuery {
    pub context: Context,
    pub expression: NodeQueryExpression,
}

#[derive(strum::EnumDiscriminants)]
#[strum_discriminants(name(NodeQueryCombinator))]
pub enum NodeQueryExpression {
    All(Vec<NodeQueryPredicate>),
    Any(Vec<NodeQueryPredicate>),
    None(Vec<NodeQueryPredicate>),
}

/// Basic Query kinds for the database. Queries are over the set of all
/// nodes.
///
/// This is an ambitious feature that is now part of the core functionality due
/// to user demand. Queries are deliberately divorced from higher-level concepts
/// such as types to keep it simple.
///
/// *Note:* When [`QueryFieldPathComponent::Any`] is present in the field path,
/// the results are combined according to [`NodeQueryCombinator`]; that is,
/// minterms is `All`; maxterms if `Any`; minterms of logical inverse otherwise.
#[derive(EnumIs, EnumTryAs, Clone)]
pub enum NodeQueryPredicate {
    // Field-related Query Primitives. All imply that such field path exists
    FieldExists(FieldQuery),
    FieldKind(FieldQuery, PayloadKind),
    FieldEqual(FieldQuery, Payload),
    FieldIsStringAndMatches(FieldQuery, QueryRegex),
    FieldIsSequenceAndContains(FieldQuery, Payload),
    FieldIsSequenceAndElementCountIs(FieldQuery, usize),
    FieldIsOrderedSetAndContains(FieldQuery, Payload),
    FieldIsOrderedSetAndElementCountIs(FieldQuery, usize),

    /// A weird one out of the bunch. If path does not contain a wildcard, the
    /// count is either zero or one. I'm not even sure what its syntax could be
    /// like!
    FieldQueryResultCount(FieldQuery, usize),

    // Related to Node itself
    Fresh,
    NodeType(String),
    HasOutgoingEdgeOfType(String),
    HasIncomingEdgeOfType(String),
    HasFreshOutgoingEdgeOfType(String),
    HasFreshIncomingEdgeOfType(String),
    Not(Box<Self>),
}

/// Path to a Node field.
///
/// Under the current data model, a node field can have arbitrary levels of
/// nested Aggregates.
#[derive(Clone)]
pub struct FieldQuery {
    field_name: String,
    path: Vec<FieldQueryPathComponent>,
}

#[derive(PartialEq, Eq, EnumIs, EnumTryAs, Clone)]
pub enum FieldQueryPathComponent {
    Last,
    Wildcard,
    Index(usize),
}

// TODO this particular flavor of regex may not be compatible with a future
// database backend
pub use regex::Regex as QueryRegex;

#[derive(PartialEq, Eq, EnumIs, EnumTryAs, Clone)]
pub enum QueryComparison {
    Lt,
    Gt,
    Ge,
    Le,
    Eq,
}

/// The primary trait describing the interface to the database.
///
/// Mutations are committed only after the resulting database passes basic
/// validation. Ordinary mutations therefore fail while a mock suffix is
/// pending, without changing the database.
pub trait LifeCore: Send + Sync {
    /// Get last event in LifeCore as a bounded Context
    fn last(&self) -> Option<EntryContext>;

    /// Total event count; no index for any event is larger than or equal to
    /// this number.
    ///
    /// *Note:* needing this count is almost always an anti-pattern, especially
    /// in an Analyzer implementation. It is not removed because someone tried
    /// to get this number by iterating over every single entry!
    #[deprecated]
    fn total_event_count(&self) -> usize;

    /// Check for the basic rules of LifeCore.
    ///
    /// # Rules
    /// TODO
    fn basic_validation(&self) -> Result<(), BasicValidationFailure>;

    /// Add a provisional edge to the validated mock suffix.
    ///
    /// A validation failure leaves the database unchanged.
    fn connect_mock(
        &mut self,
        from: EntryContext,
        ty: &str,
        to: EntryContext,
    ) -> Result<Edge, LifeCoreError>;

    /// Add a provisional node to the validated mock suffix.
    ///
    /// A validation failure leaves the database unchanged.
    fn add_mock(
        &mut self,
        ty: &str,
        payload: Body,
        parent: Option<EntryContext>,
    ) -> Result<Node, LifeCoreError>;

    /// Whether this database contains a validated suffix of mock events.
    fn is_mock(&self) -> bool;

    /// Replace the `Mock` circumstances with the current session information,
    /// queried at the moment of this method's invocation.
    ///
    /// This is a no-op when no mock events are present. A failure leaves the
    /// database unchanged.
    fn apply(&mut self) -> Result<(), LifeCoreError>;

    /// Get the event selected by `e`. Preserves the context across invocations.
    ///
    /// *Note:* We used to also do type resolution for callers, but as typecheck
    /// grew away from the main crate, it is no longer critical to get the
    /// window exactly right; still, it is recommended that you use the best
    /// context you can for the event to maintain consistency with Analyzers and
    /// minimize confusion.
    fn get(&self, e: EntryContext) -> Option<Entry>;
    fn get_with_circumstance(&self, e: EntryContext) -> Option<(Entry, Circumstance)>;
    fn window<I>(&self, i: I) -> impl Iterator<Item = Entry>
    where
        I: IntoIterator<Item = EntryContext>;
    /// Check whether `e.thing` is stale within context. Return None for invalid
    /// input. Stale Node means a newer node has it as its parent within
    /// context. Stale Edge means at least one end is a Stale Node
    fn is_stale(&self, e: EntryContext) -> Option<bool>;

    /// Find all Entries that pass **all** [`Query`] predicates
    fn query(&self, query: NodeQuery) -> Vec<Node>;

    /// Get node at the specified index and context, `None` if is not a node or
    /// is out-of-bounds. See also: [`LifeCore::get`]
    fn node(&self, ctx: EntryContext) -> Option<Node> {
        if let Some(Entry::Node(n)) = self.get(ctx) {
            Some(n)
        } else {
            None
        }
    }

    /// Get edge at the specified index and context, `None` if is not a edge or
    /// is out-of-bounds. See also: [`LifeCore::get`]
    fn edge(&self, ctx: EntryContext) -> Option<Edge> {
        if let Some(Entry::Edge(e)) = self.get(ctx) {
            Some(e)
        } else {
            None
        }
    }

    fn nodes(&self, ctx: Context) -> impl Iterator<Item = Node>;
    fn edges(&self, ctx: Context) -> impl Iterator<Item = Edge>;

    fn fresh_nodes(&self, ctx: Context) -> impl Iterator<Item = Node> {
        self.nodes(ctx).filter(|x| !self.is_stale(x.e).unwrap())
    }
    fn fresh_edges(&self, ctx: Context) -> impl Iterator<Item = Edge> {
        self.edges(ctx).filter(|x| !self.is_stale(x.e).unwrap())
    }

    /// Get non-stale edges connected to node `n`
    ///
    /// *Note:* for this function to return anything meaningful, `n` itself
    /// needs to be fresh within the context that implies. That is, if `end` is
    /// far enough past the index so that `n` is stale, any edge connected to
    /// it is also stale.
    ///
    /// # Example
    /// ```rust
    /// # use lifecore::{LifeCoreList, LifeCore, NodeBodyExt, Node, Edge};
    /// let mut lc = LifeCoreList::default();
    /// let a0 = lc.add("A", NodeBodyExt::default(), None).unwrap();
    /// let b0 = lc.add("B", NodeBodyExt::default(), None).unwrap();
    /// let _ = lc.connect(a0.e, "X", b0.e).unwrap();
    /// let b1 = lc.add("B", NodeBodyExt::default(), Some(b0.e)).unwrap();
    /// let _ = lc.connect(a0.e, "X", b1.e).unwrap();
    /// assert_eq!(lc.fresh_edges_of(b0.e).count(), 0);
    /// ```
    fn fresh_edges_of(&self, n: EntryContext) -> impl Iterator<Item = Edge> {
        self.edges(n.window()).filter(move |x| {
            !self.is_stale(n.look_at(x.e)).unwrap()
                && (x.from.absolute() == n.absolute() || x.to.absolute() == n.absolute())
        })
    }

    /// Get fresh outgoing edges from `n` within the implied context. See
    /// [`LifeCore::fresh_edges_of`]
    ///
    /// Outgoing edge means an edge where `n` is `from`, also known as the head
    /// of the edge.
    fn fresh_outgoing_edges_of(&self, n: EntryContext) -> impl Iterator<Item = Edge> {
        self.edges(n.window())
            .filter(move |x| !self.is_stale(x.e).unwrap() && x.from.absolute() == n.absolute())
    }

    /// Get fresh Incoming edges from `n` within the implied context. See
    /// [`LifeCore::fresh_edges_of`]
    ///
    /// Incoming edge means an edge where `n` is `to`, also known as the tail of
    /// the edge
    fn fresh_incoming_edges_to(&self, n: EntryContext) -> impl Iterator<Item = Edge> {
        self.edges(n.window())
            .filter(move |x| !self.is_stale(x.e).unwrap() && x.to.absolute() == n.absolute())
    }

    /// Get root of node at `e`. That is, the ancestor node by the way of
    /// `parent` component
    fn root_of(&self, e: EntryContext) -> Option<EntryContext>;

    /// Returns fresh successors within context.
    /// A 'successor' is defined as any non-stale node within context that has
    /// `node` as its ancestor. An 'ancestor' of a node is any node within
    /// `.parent` chain of it. If a node has no parent, it is its own ancestor.
    ///
    /// *Note:* Only searches within the context.
    fn fresh_successors(&self, node: EntryContext) -> Vec<EntryContext>;

    /// Find relative degree. Siblings (nodes that share a parent) have degree
    /// 0. Note that the left sides of the contexts need to extend far back
    /// enough in order to find the shared parent.
    fn relative_degree(&self, a: EntryContext, b: EntryContext) -> Option<usize>;
    fn are_siblings(&self, a: EntryContext, b: EntryContext) -> bool {
        self.relative_degree(a, b) == Some(0)
    }
    fn families(&self, ctx: Context) -> Vec<LifeCoreFamily>;
    fn add_analyzer(&mut self, name: &str, config: &str);
    fn current_analysis_suite(&self) -> analysis::SuiteConfig;
}

/// Reference implementor of LifeCore
pub use reference_impl::LifeCoreList;

pub mod analysis;
mod boilerplate;
mod reference_impl;
#[cfg(test)]
mod test;
