use itertools::Itertools;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    collections::HashMap,
    fmt::Display,
    hash::Hash,
    ops::{Index, Range},
};
use strum::{EnumIs, EnumTryAs};

mod internal;
#[cfg(test)]
mod test;
mod typing;

// Used for "external" convenient usage
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct NodeBodyExt(pub HashMap<String, PayloadExt>);

pub use jiff::Zoned;

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
    SystemsError(AnalysisOutput), // TODO how to only report Inconsistency and BasicValidationError instead
}

pub use jiff::Timestamp;

pub use internal::PayloadType;

// This is a convenient type for the user
#[derive(
    Serialize,
    Deserialize,
    Clone,
    PartialEq,
    Eq,
    Hash,
    strum::VariantNames,
    strum::EnumIs,
    strum::EnumTryAs,
    Debug,
)]
pub enum PayloadExt {
    PreciseTime(Timestamp),
    Text(String),
    Number(i64),

    // NOTE this should be a last resort for users. As time goes on, we expand
    // the list of primitve types to further minimize its use.
    // Serialized LOBs are discouraged.
    Serialized(serde_json::Value),
}

impl PayloadExt {
    pub fn serialized_from<T: Serialize>(x: T) -> Self {
        Self::Serialized(serde_json::to_value(x).unwrap())
    }
}

// TODO this may have been a bad decision: Type depends on the context and can evolve,
// so now use an `EventContext` to the type to keep track of at which point in time we meant it.
// Of course, this adds another validity issue: corresponding EventIndex must be valid in this
// window!
// NOTE `ty` may fail to resolve too! so just in case we also include `ty_name`
// _Usually_, ty.is_none() indicates a bigger problem but dealing with it
// is not the responsibility of users, for now.
#[derive(Clone, PartialEq, Eq)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub struct NodeExt {
    pub e: EventContext,
    pub ty: Option<NodeType>,
    pub ty_name: String,
    pub field: NodeBodyExt,
    pub parent: Option<EventContext>,
}

#[derive(Clone, PartialEq, Eq)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub struct EdgeExt {
    pub e: EventContext,
    pub ty_name: String,
    pub from: EventContext,
    pub to: EventContext,
}

#[derive(EnumIs, EnumTryAs, Clone, PartialEq, Eq)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub enum EventExt {
    Node(NodeExt),
    Edge(EdgeExt),
}

#[derive(PartialEq, Eq, Clone, EnumIs, Debug)]
pub enum AnalysisReport {
    BasicValidationFailure {
        offending_event: EventContext,
        reason: BasicValidationFailureScenario,
    },
    BasicValidationWarning {
        offending_event: EventContext,
        message: String,
    },
    LogInfo {
        offending_event: EventContext,
        message: String,
    },
    // Errors from Constraint systems
    Inconsistency(ConstraintSystemMessage),
    // Information that ought to be acted upon.
    Warning(ConstraintSystemMessage),
    // Actual suggestions for things to add. Must be a concrete action (add this node, or edge, or delete this)
    Suggestion(ConstraintSystemMessage, SuggestedAction),
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct AnalysisOutput(pub Vec<AnalysisReport>);

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct ConstraintSystemMessage {
    // Contains the event that was problematic and the neighborhood that the error happened
    pub offending_event: EventContext,
    pub system_name: String,
    pub system_config: String,
    // Errors are always relevant, of course, but warnings less so because we may have resolved them in a future window.
    // The UI may choose to ignore this message if N members of `relevance` are stale (for some sensible N)
    // Additionally, UI may implement a `barklist` where messages of a certain hash are dismissed.
    // (This led to needing a custom Hash implementation that specifically sorts this list)
    pub relevance: Vec<EventContext>,
    pub message: String,
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum BasicValidationFailureScenario {
    InternalError { message: String },
    ExtraneousMockCircumstance,
    DuplicateEdge,
    DuplicateNode,
    StaleRuleViolation,
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub enum SuggestedAction {
    AddNode {
        ty_name: String,
        payload: NodeBodyExt,
        parent: Option<EventContext>,
    },
    AddEdge {
        from: EventContext,
        ty_name: String,
        to: EventContext,
    },
}

// Implementation Rules:
// - Don't be too high-level, and use EventContext instead of heavy-weight NodeExt and EdgeExt as parameters
// - Expect duplicate code; Leverage the private helpers on the implementor type
pub trait LifeCore {
    // TODO assert for boundedness
    /// Get last event in LifeCore as a bounded EventContext
    fn last(&self) -> Option<EventContext>;
    fn connect(
        &mut self,
        from: EventContext,
        ty: &str,
        to: EventContext,
    ) -> Result<EdgeExt, LifeCoreError>;
    fn add(
        &mut self,
        ty: &str,
        payload: NodeBodyExt,
        parent: Option<EventContext>,
    ) -> Result<NodeExt, LifeCoreError>;
    fn get_or_insert_current_session_circumstance(
        &mut self,
    ) -> Result<CircumstanceExt, LifeCoreError>;
    fn get_or_insert_mock_circumstance(&mut self) -> Result<CircumstanceExt, LifeCoreError>;
    fn get(&self, e: EventContext) -> Option<EventExt>;
    fn window<I>(&self, i: I) -> impl Iterator<Item = EventExt>
    where
        I: IntoIterator<Item = EventContext>;
    /// Check whether `e.thing` is stale within context
    /// Stale Node means a newer node has it as its parent within context
    /// Stale Edge means at least one end is a Stale Node
    fn is_stale(&self, e: EventContext) -> bool;
    fn basic_validation(&self) -> AnalysisOutput;
    // NOTE retrieves nodes with their types resolved within the context. Convenient!
    // However! NOTE that the type may not actually be what was intended when the node was created
    fn node(&self, ctx: EventContext) -> NodeExt; // NOTE panics if ctx doesn't denote a Node
    fn edge(&self, ctx: EventContext) -> EdgeExt; // NOTE panics similarly
    fn nodes(&self, ctx: EventContext) -> impl Iterator<Item = NodeExt>; // NOTE panics similarly
    fn edges(&self, ctx: EventContext) -> impl Iterator<Item = EdgeExt>; // NOTE panics similarly
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
    /// let mut lc = LifeCoreList::new();
    /// let a0 = lc.add("A", NodeBodyExt::default(), None).unwrap();
    /// let b0 = lc.add("B", NodeBodyExt::default(), None).unwrap();
    /// let _ = lc.connect(a0.e, "X", b0.e).unwrap();
    /// let b1 = lc.add("B", NodeBodyExt::default(), Some(b0.e)).unwrap();
    /// let _ = lc.connect(a0.e, "X", b1.e).unwrap();
    /// assert_eq!(lc.fresh_edges_of(b0).count(), 0);
    /// ```
    fn fresh_edges_of(&self, n: EventContext) -> impl Iterator<Item = EdgeExt> {
        let ctx = n.exclusive_right_context();
        self.edges(ctx).filter(move |x| {
            !self.is_stale(x.e)
                && (x.from.absolute() == n.absolute() || x.to.absolute() == n.absolute())
        })
    }

    /// Get fresh outgoing edges from `n` within the implied context.
    /// 
    /// Outgoing edge means an edge where `n` is `from`, also known as the head of the edge
    /// 
    /// *Note:* for this function to return anything meaningful, `n` itself
    /// needs to be fresh within the context that implies. That is, if `n` is
    /// stale, any edge connected to it is also stale.
    fn fresh_outgoing_edges_of(&self, n: EventContext) -> impl Iterator<Item = EdgeExt> {
        let ctx = n.exclusive_right_context();
        self.edges(ctx)
            .filter(move |x| !self.is_stale(x.e) && x.from.absolute() == n.absolute())
    }

    /// Get fresh Incoming edges from `n` within the implied context.
    /// 
    /// Incoming edge means an edge where `n` is `to`, also known as the tail of the edge
    /// 
    /// *Note:* for this function to return anything meaningful, `n` itself
    /// needs to be fresh within the context that implies. That is, if `n` is
    /// stale, any edge connected to it is also stale.
    fn fresh_incoming_edges_to(&self, n: EventContext) -> impl Iterator<Item = EdgeExt> {
        let ctx = n.exclusive_right_context();
        self.edges(ctx)
            .filter(move |x| !self.is_stale(x.e) && x.to.absolute() == n.absolute())
    }

    /// Get root of node at `e`. That is, the ancestor node by the way of `parent` component
    fn root_of(&self, e: EventContext) -> Option<EventContext>;

    /// Returns fresh successors within context. Use `.absolute()` to get current fresh successors
    fn fresh_successors(&self, node: EventContext) -> Vec<EventContext>;

    /// Find relative degree. Siblings (nodes that share a parent) have degree 0.
    /// Note that the left sides of the contexts need to extend far back enough in order to find the shared parent.
    fn relative_degree(&self, a: EventContext, b: EventContext) -> Option<usize>;
    fn are_siblings(&self, a: EventContext, b: EventContext) -> bool {
        self.relative_degree(a, b) == Some(0)
    }
    /// Return all the fresh types in context
    fn types(&self, e: EventContext) -> impl Iterator<Item = (String, (EventContext, NodeType))>;
    fn non_solo_families(&self, ctx: EventContext) -> Vec<Vec<EventContext>>;
    fn add_analyzer(&mut self, name: &str, config: &str);
    fn current_analysis_suite(&self) -> AnalysisSuiteConfig;
}

/// Canonical implementor of LifeCore
pub use internal::LifeCoreList;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnalysisSuiteConfig(pub Vec<AnalysisConfig>);

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnalysisConfig {
    pub name: String,
    pub config: String,
}

// The idea behind `initialize` is that if some pre-condition is not quite correct, or
// if we are "done" in some sense, analyzer can disable itself and maybe issue a warning.
pub trait Analyzer<LC: LifeCore>: Send + Sync {
    fn initialize(&mut self, lc: &LC) -> AnalysisOutput;
    fn analyze(&self, lc: &LC) -> AnalysisOutput;
}

pub trait AnalyzerFactory<LC: LifeCore> {
    fn analyzer_with_config(
        &mut self,
        config: &str,
    ) -> Result<Box<dyn Analyzer<LC>>, LifeCoreError>;
    fn analyzer_name(&self) -> &'static str;
}

pub struct AnalysisRunner<LC: LifeCore> {
    factory: HashMap<String, Box<dyn AnalyzerFactory<LC>>>,
    analyzer: Vec<Box<dyn Analyzer<LC>>>,
}

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

    pub fn is_node_and<F: Fn(&NodeExt) -> bool>(&self, f: F) -> bool {
        self.is_node() && f(&self.as_node())
    }

    pub fn is_edge_and<F: Fn(&EdgeExt) -> bool>(&self, f: F) -> bool {
        self.is_edge() && f(&self.as_edge())
    }

    pub fn ctx(&self) -> EventContext {
        match self {
        EventExt::Node(node_ext) => node_ext.e,
        EventExt::Edge(edge_ext) => edge_ext.e,
        }
    }
}

impl AnalysisOutput {
    pub fn has_error(&self) -> bool {
        for x in self.0.iter() {
            match x {
            AnalysisReport::BasicValidationFailure { .. } => return true,
            AnalysisReport::LogInfo { .. } => continue,
            AnalysisReport::Inconsistency(_) => return true,
            AnalysisReport::Warning(_) => continue,
            AnalysisReport::Suggestion(_, _) => continue,
            AnalysisReport::BasicValidationWarning { .. } => continue,
            }
        }
        false
    }

    pub fn err(&self) -> LifeCoreError {
        assert!(self.has_error());
        LifeCoreError::SystemsError(AnalysisOutput(
            self.0
                .iter()
                .filter(|x| x.is_basic_validation_failure() || x.is_inconsistency())
                .cloned()
                .collect_vec(),
        ))
    }

    pub fn is_success(&self) -> bool {
        !self.has_error()
    }

    pub fn has_warning(&self) -> bool {
        for x in self.0.iter() {
            match x {
            AnalysisReport::BasicValidationFailure { .. } => continue,
            AnalysisReport::LogInfo { .. } => continue,
            AnalysisReport::Inconsistency(_) => continue,
            AnalysisReport::Warning(_) => return true,
            AnalysisReport::Suggestion(_, _) => continue,
            AnalysisReport::BasicValidationWarning { .. } => continue,
            }
        }
        false
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Copy, Hash, Debug)]
pub struct EventContext {
    // Inclusive start.
    // By default, None means "since the beginning"
    // NOTE rarely if ever we'll need to constrain the start. Perhaps later when optimizing?
    pub(crate) start: Option<usize>,
    // Exclusive! `start..end` basically
    // Used to calculate type, check for staleness etc
    // By default, None means "till forever" (goes to infinity)
    pub(crate) end: Option<usize>,

    // What was the object of inspection?
    // Ought to be in start..end
    pub(crate) thing: Option<usize>,
}

impl Ord for EventContext {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        assert!(
            self.validate() && self.thing.is_some() && other.validate() && other.thing.is_some()
        );
        self.thing.unwrap().cmp(&other.thing.unwrap())
    }
}

impl PartialOrd for EventContext {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(&other))
    }
}

impl EventContext {
    pub(crate) fn range(&self) -> Range<usize> {
        self.start.unwrap_or(0)..self.end.unwrap_or(usize::MAX)
    }

    pub fn validate(&self) -> bool {
        self.thing.is_none() || self.range().contains(&self.thing.unwrap())
    }

    pub(crate) fn from_range(r: Range<usize>, thing: Option<usize>) -> Self {
        Self {
            start: Some(r.start),
            end: Some(r.end),
            thing,
        }
    }

    pub fn absolute(self) -> Self {
        Self::universe().look_at(self)
    }

    pub fn last(&self) -> EventContext {
        assert!(self.is_bounded() && !self.is_empty() && self.validate());
        self.look_raw(self.end.unwrap() - 1)
    }

    pub fn first(&self) -> EventContext {
        assert!(!self.is_empty() && self.validate());
        self.look_raw(self.start.unwrap_or(0))
    }

    // Look at every event within context, from the context
    // NOTE potentially unbounded!
    pub fn items(self) -> impl Iterator<Item = Self> {
        assert!(self.validate());
        let ctx = self;
        self.range().map(move |x| ctx.look_raw(x))
    }

    pub fn inclusive_right_context(self) -> Self {
        assert!(self.validate() && self.thing.is_some());
        let r = Self {
            start: self.thing,
            end: self.end,
            thing: Some(self.thing.unwrap()),
        };
        assert!(r.validate());
        r
    }

    pub fn inclusive_right(self) -> impl Iterator<Item = Self> {
        self.inclusive_right_context().items()
    }

    pub fn inclusive_left_context(&self) -> Self {
        assert!(self.validate() && self.thing.is_some());
        // We know that thing is in start..end (that is, thing.is_some() and the range non-empty)
        // We can safely increment self.thing and use it as the upper bound
        let r = Self {
            start: self.start,
            end: Some(self.thing.unwrap() + 1),
            thing: None,
        };
        assert!(r.validate());
        r
    }

    pub fn inclusive_left(self) -> impl Iterator<Item = Self> {
        self.inclusive_left_context().items()
    }

    pub fn exclusive_right_context(self) -> Self {
        assert!(self.validate() && self.thing.is_some());
        let r = Self {
            start: Some(self.thing.unwrap() + 1),
            end: self.end,
            thing: None,
        };
        assert!(r.validate());
        r
    }

    pub fn exclusive_right(self) -> impl Iterator<Item = Self> {
        self.exclusive_right_context().items()
    }

    pub fn exclusive_left_context(&self) -> Self {
        assert!(self.validate() && self.thing.is_some());
        // We know that thing is in start..end (that is, thing.is_some() and the range non-empty)
        // We can safely pass self.thing as the upper bound
        let r = Self {
            start: self.start,
            end: self.thing,
            thing: None,
        };
        assert!(r.validate());
        r
    }

    pub fn exclusive_left(self) -> impl Iterator<Item = Self> {
        self.exclusive_left_context().items()
    }

    pub fn is_bounded(&self) -> bool {
        assert!(self.validate());

        self.end.is_some()
    }

    pub fn is_empty(&self) -> bool {
        assert!(self.validate());

        self.range().is_empty()
    }

    // Builders
    // Take care to write an interface that minimizes usage mistakes

    /// NOTE throws away the context on `e.thing()` and looks at it from `self` perspective
    pub fn look_at(&self, e: EventContext) -> Self {
        assert!(self.validate());
        let mut r = *self;
        r.thing = e.thing;
        assert!(r.validate());
        r
    }

    // NOTDO this is the only function that somewhat leaks implementation details.
    // Handle with care!
    pub(crate) fn look_raw(&self, e: usize) -> Self {
        assert!(self.validate());
        let mut r = *self;
        r.thing = Some(e);
        assert!(r.validate());
        r
    }

    pub fn intersect(&self, rhs: &Self) -> Self {
        assert!(self.validate() && rhs.validate() && self.thing == rhs.thing);
        let start = std::cmp::max(self.start.unwrap_or(0), rhs.start.unwrap_or(0));
        let end = std::cmp::min(
            self.end.unwrap_or(usize::MAX),
            rhs.end.unwrap_or(usize::MAX),
        );
        Self {
            start: Some(start),
            end: Some(end),
            thing: self.thing,
        }
    }

    pub fn universe() -> Self {
        Self {
            start: None,
            end: None,
            thing: None,
        }
    }
}

// TODO
// impl IntoIterator<Item> for EventContext {
//     type Item=EventContext;

//     type IntoIter;

//     fn into_iter(self) -> Self::IntoIter {
//         todo!()
//     }
// }

impl<LC: LifeCore> AnalysisRunner<LC> {
    pub fn new() -> Self {
        Self {
            factory: HashMap::new(),
            analyzer: vec![],
        }
    }

    pub fn register_analyzer_kind(
        &mut self,
        factory: Box<dyn AnalyzerFactory<LC>>,
    ) -> Result<(), LifeCoreError> {
        if self.factory.contains_key(factory.analyzer_name()) {
            return Err(LifeCoreError::DuplicateAnalyzer {
                dupe_name: factory.analyzer_name().to_string(),
            });
        }
        self.factory
            .insert(factory.analyzer_name().to_string(), factory);
        Ok(())
    }

    pub fn run(&mut self, lc: &LC) -> Result<AnalysisOutput, LifeCoreError> {
        let mut analyzer: Vec<Box<dyn Analyzer<LC>>> = vec![];
        for x in lc.current_analysis_suite().0.iter() {
            if self.factory.contains_key(&x.name) {
                analyzer.push(
                    self.factory
                        .get_mut(&x.name)
                        .unwrap()
                        .analyzer_with_config(&x.config)?,
                );
            } else {
                return Err(LifeCoreError::AnalyzerNotFound {
                    name: x.name.clone(),
                });
            }
        }
        let mut r: Vec<AnalysisReport> =
            analyzer
                .iter_mut()
                .map(|x| x.initialize(lc).0)
                .fold(vec![], |acc, x| {
                    let mut t = vec![];
                    t.extend(acc);
                    t.extend(x);
                    t
                });
        r.extend(
            analyzer
                .into_iter()
                .map(|x| x.analyze(lc).0)
                .fold(vec![], |acc, x| {
                    let mut t = vec![];
                    t.extend(acc);
                    t.extend(x);
                    t
                }),
        );
        Ok(AnalysisOutput(r))
    }
}

impl ConstraintSystemMessage {
    pub fn validate(&self) -> bool {
        self.offending_event.validate() && self.offending_event.thing.is_some()
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
            Result::<Timestamp, LifeCoreError>::Err(LifeCoreError::FieldNotFound {
                field_name: name.to_string(),
            })
        } else if !self.0[name].is_precise_time() {
            Result::<Timestamp, LifeCoreError>::Err(LifeCoreError::FieldContainsUnexpectedType {
                field_name: name.to_string(),
            })
        } else {
            Result::<Timestamp, LifeCoreError>::Ok(
                self.0[name].clone().try_as_precise_time().unwrap(),
            )
        }
    }
    pub fn get_number(&self, name: &str) -> Result<i64, LifeCoreError> {
        if !self.0.contains_key(name) {
            Result::<i64, LifeCoreError>::Err(LifeCoreError::FieldNotFound {
                field_name: name.to_string(),
            })
        } else if !self.0[name].is_number() {
            Result::<i64, LifeCoreError>::Err(LifeCoreError::FieldContainsUnexpectedType {
                field_name: name.to_string(),
            })
        } else {
            Result::<i64, LifeCoreError>::Ok(self.0[name].clone().try_as_number().unwrap())
        }
    }
    pub fn get_text(&self, name: &str) -> Result<String, LifeCoreError> {
        if !self.0.contains_key(name) {
            Result::<String, LifeCoreError>::Err(LifeCoreError::FieldNotFound {
                field_name: name.to_string(),
            })
        } else if !self.0[name].is_text() {
            Result::<String, LifeCoreError>::Err(LifeCoreError::FieldContainsUnexpectedType {
                field_name: name.to_string(),
            })
        } else {
            Result::<String, LifeCoreError>::Ok(self.0[name].clone().try_as_text().unwrap())
        }
    }

    pub fn get_deserialized<T: DeserializeOwned>(&self, name: &str) -> Result<T, LifeCoreError> {
        if !self.0.contains_key(name) {
            Result::<T, LifeCoreError>::Err(LifeCoreError::FieldNotFound {
                field_name: name.to_string(),
            })
        } else if !self.0[name].is_serialized() {
            Result::<T, LifeCoreError>::Err(LifeCoreError::FieldContainsUnexpectedType {
                field_name: name.to_string(),
            })
        } else {
            if let Result::Ok(x) =
                serde_json::from_value::<T>(self.0[name].clone().try_as_serialized().unwrap())
            {
                Result::<T, LifeCoreError>::Ok(x)
            } else {
                Result::<T, LifeCoreError>::Err(LifeCoreError::FieldDeserializationFailed {
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

// Special-Case Hash: relevance list should be sorted first
impl Hash for ConstraintSystemMessage {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.offending_event.hash(state);
        self.system_name.hash(state);
        self.system_config.hash(state);
        {
            let mut sorted = self.relevance.clone();
            sorted.sort();
            sorted.hash(state);
        }
        self.message.hash(state);
    }
}

impl Display for PayloadExt {
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

impl Display for EventContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(
            format!(
                "EventContext[{}, <{}>, {})",
                self.start.unwrap_or(0),
                {
                    if let Some(x) = self.thing {
                        format!("{}", x)
                    } else {
                        "NONE".to_string()
                    }
                }
                .as_str(),
                {
                    if let Some(x) = self.end {
                        format!("{}", x)
                    } else {
                        "INF".to_string()
                    }
                }
                .as_str()
            )
            .as_str(),
        )
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

impl Default for NodeBodyExt {
    fn default() -> Self {
        Self(HashMap::new())
    }
}
