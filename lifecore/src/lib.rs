use indexmap::IndexSet;
use itertools::Itertools;
use jiff::{Timestamp, Zoned};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{collections::HashMap, fmt::Display, hash::Hash, ops::{Index, Range}};
use strum::{EnumIs, EnumTryAs};
use thiserror;

use crate::analysis::AnalysisSuiteConfig;
pub use crate::lifecore::*;
use crate::typecheck::NodeType;

// Used for "internal" representation of NodeBody
#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Copy, Hash)]
pub(crate) struct NodeBody([Option<(StringIndex, Payload)>; 32]);

// Used for "external" convenient usage
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub struct NodeBodyExt(pub HashMap<String, PayloadExt>);

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
pub(crate) struct CircumstanceIndex(u64);

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
pub(crate) struct StringIndex(u64);

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
pub(crate) struct EventIndex(pub u64);

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]

pub struct EventContext {
    // Inclusive start.
    // By default, None means "since the beginning"
    // NOTE rarely if ever we'll need to constrain the start. Perhaps later when optimizing?
    start: Option<usize>,
    // Exclusive! `start..end` basically
    // Used to calculate type, check for staleness etc
    // By default, None means "till forever" (goes to infinity)
    end: Option<usize>,

    // What was the object of inspection?
    // Ought to be in start..end
    thing: Option<usize>,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Hash)]
pub enum Circumstance {
    Mock,
    Session {
        when_1h: Zoned,
        hostname: StringIndex,
        username: StringIndex,
    },
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct LifeCore {
    // indexed by EventIndex
    event: Vec<Event>,
    circumstance: Vec<CircumstanceIndex>,

    // indexed by CircumstanceIndex
    circumstance_table: IndexSet<Circumstance>,

    // indexed by StringIndex
    string_table: IndexSet<String>,

    // Analyzers added so far. An analyzer has 2 pieces of state:
    // - initial config (just a small short string)
    // - nodes and edges that may get added as part of encouragement through AnalysisReport (may indicate "progression")
    configured_analyzer: AnalysisSuiteConfig,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, thiserror::Error)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub enum LifeCoreError {
    #[error(
        "Field {field_name:?} Couldn't Be Deserialized! (either to JSON or to the target type)"
    )]
    FieldDeserializationFailed { field_name: String },
    #[error("Field {field_name:?} was not found!")]
    FieldNotFound { field_name: String },
    #[error("Field {field_name:?} is not the type we expected it to be")]
    FieldContainsUnexpectedType { field_name: String },
    #[error("Potentially different analyzers under the same name {dupe_name:?}")]
    DuplicateAnalyzer { dupe_name: String },
    #[error("Analyzer {name:?} not found")]
    AnalyzerNotFound { name: String },
}

#[derive(Serialize, Deserialize, EnumIs, EnumTryAs, Clone, PartialEq, Eq, Hash)]
pub(crate) enum Event {
    Node(Node),
    Edge(Edge),
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Node {
    pub ty: StringIndex,
    pub parent: Option<EventIndex>,
    pub payload: NodeBody,
}
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Edge {
    pub ty: StringIndex,
    pub from: EventIndex,
    pub to: EventIndex,
}

// This is for the internal representation of Node Payload
#[derive(
    Serialize,
    Deserialize,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    strum::VariantNames,
    strum::EnumDiscriminants,
)]
#[strum_discriminants(name(PayloadType))]
#[strum_discriminants(vis(pub))]
#[strum_discriminants(derive(Serialize, Deserialize, Hash))]
pub(crate) enum Payload {
    PreciseTime(Timestamp),
    Text(StringIndex),
    Number(i64),

    // NOTE this should be a last resort for users. As time goes on, we may need to expand
    // the list of primitve types. Serialized LOBs are discouraged.
    Serialized(StringIndex),
}

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
)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub enum PayloadExt {
    PreciseTime(Timestamp),
    Text(String),
    Number(i64),

    // NOTE this should be a last resort for users. As time goes on, we may need to expand
    // the list of primitve types. Serialized LOBs are discouraged.
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
pub struct NodeExt {
    e: EventContext,
    ty: Option<NodeType>,
    ty_name: String,
    field: NodeBodyExt,
    parent: Option<EventIndex>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct EdgeExt {
    e: EventContext,
    ty_name: String,
    from: EventContext,
    to: EventContext,
}

#[derive(EnumIs, EnumTryAs, Clone, PartialEq, Eq)]
pub enum EventExt {
    Node(NodeExt),
    Edge(EdgeExt),
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum AnalysisReport {
    BasicValidationFailure {
        offending_event: EventContext,
        reason: BasicValidationFailureScenario,
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

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct AnalysisOutput(pub Vec<AnalysisReport>);

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone)]
pub struct ConstraintSystemMessage {
    pub offending_event: EventContext,
    pub window: Range<usize>,
    pub system_name: String,
    pub system_config: String,
    // Errors are always relevant, of course, but warnings less so because we may have resolved them in a future window.
    // The UI may choose to ignore this message if N members of `relevance` are stale (for some sensible N)
    // Additionally, UI may implement a `barklist` where messages of a certain hash are dismissed.
    // (This lead to needing a custom Hash implementation that specifically sorts this list)
    pub relevance: Vec<EventIndex>,
    pub message: String,
}

// TODO
#[derive(Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum BasicValidationFailureScenario {}

// TODO
#[derive(Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum SuggestedAction {
    AddSpecificNode(Node),
}

mod boilerplate {
    use super::*;
    impl NodeBody {
        pub fn new() -> Self {
            Self([const { None }; 32])
        }

        pub fn insert(&mut self, key: StringIndex, val: Payload) {
            let first_empty = {
                let x = self
                    .0
                    .iter()
                    .enumerate()
                    .filter(|x| x.1.is_none())
                    .take(1)
                    .collect_vec();
                if x.is_empty() { None } else { Some(x[0].0) }
            }
            .expect("Payload table has at least one empty slot.");
            self.0[first_empty] = Some((key, val));
        }

        pub fn from(tab: HashMap<StringIndex, Payload>) -> Self {
            assert!(tab.len() <= 32, "HashMap too large!");
            let mut r = Self::new();
            for x in tab.into_iter() {
                r.insert(x.0, x.1);
            }
            return r;
        }
    }

    impl NodeBodyExt {
        pub fn from(x: &[(&str, PayloadExt)]) -> Self {
            let mut r = HashMap::<String, PayloadExt>::new();
            assert!(
                x.iter().map(|y| &y.0).counts().len() == x.len(),
                "Supplied list has a repeated key (not a proper set!)"
            );
            for (k, v) in x.into_iter() {
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
                Result::<Timestamp, LifeCoreError>::Err(
                    LifeCoreError::FieldContainsUnexpectedType {
                        field_name: name.to_string(),
                    },
                )
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

        pub fn get_deserialized<T: DeserializeOwned>(
            &self,
            name: &str,
        ) -> Result<T, LifeCoreError> {
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

    impl ConstraintSystemMessage {
        pub fn new_message(&self, message: String) -> Self {
            Self {
                message,
                ..self.clone()
            }
        }
    }

    // Special-Case Hash: relevance list should be sorted first
    impl Hash for ConstraintSystemMessage {
        fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
            self.offending_event.hash(state);
            self.window.hash(state);
            self.system_name.hash(state);
            self.system_config.hash(state);
            self.relevance.clone().sort().hash(state);
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

    impl AnalysisOutput {
        // TODO
        // has_error
        // first_error
    }

    impl From<&usize> for EventIndex {
        fn from(value: &usize) -> Self {
            Self(*value as u64)
        }
    }

    impl From<usize> for EventIndex {
        fn from(value: usize) -> Self {
            Self::from(&value)
        }
    }

    impl Into<usize> for &EventIndex {
        fn into(self) -> usize {
            self.0 as usize
        }
    }

    impl Into<usize> for EventIndex {
        fn into(self) -> usize {
            (&self).into()
        }
    }

    impl Index<&str> for NodeBodyExt {
        type Output = PayloadExt;

        fn index(&self, index: &str) -> &Self::Output {
            self.0.index(index)
        }
    }

    impl EventIndex {
        pub fn before(&self) -> EventContext {
            EventContext::from(0..(self.0 as usize), Some(self.0 as usize))
        }

        pub fn before_and_including(&self) -> EventContext {
            // Can't use RangeInclusive because it can't be converted to Range easily
            EventContext::from(0..((self.0 + 1) as usize), Some(self.0 as usize))
        }
    }

    impl EventContext {
        fn range(&self) -> Range<usize> {
            self.start.unwrap_or(0)..self.end.unwrap_or(usize::MAX)
        }

        fn validate(&self) -> bool {
            self.thing.is_none() || self.range().contains(&self.thing.unwrap())
        }

        pub(in crate::boilerplate) fn from(r: Range<usize>, thing: Option<usize>) -> Self {
            Self {
                start: Some(r.start),
                end: Some(r.end),
                thing,
            }
        }

        pub fn last(&self) -> EventIndex {
            assert!(self.is_bounded() && !self.is_empty() && self.validate());
            return EventIndex::from(self.end.unwrap() - 1);
        }

        pub fn first(&self) -> EventIndex {
            assert!(!self.is_empty() && self.validate());
            return EventIndex::from(self.start.unwrap_or(0));
        }

        pub fn into_iter(self) -> impl Iterator<Item = EventIndex> {
            assert!(self.validate());
            self.range().into_iter().map(|x| EventIndex::from(x))
        }

        pub fn is_bounded(&self) -> bool {
            assert!(self.validate());

            self.end.is_some()
        }

        pub fn is_empty(&self) -> bool {
            assert!(self.validate());

            self.range().is_empty()
        }

        pub fn look(&self, e: EventIndex) -> Self {
            assert!(self.validate());
            let mut r = self.clone();
            r.thing = Some(e.0 as usize);
            assert!(r.validate());
            r
        }

        pub fn intersect(&self, rhs: &Self) -> Self {
            assert!(self.validate() && rhs.validate() && self.thing == rhs.thing);
            let start = std::cmp::min(self.start.unwrap_or(0), rhs.start.unwrap_or(0));
            let end = std::cmp::min(self.end.unwrap_or(0), rhs.end.unwrap_or(0));
            Self {
                start: Some(start),
                end: Some(end),
                thing: self.thing,
            }
        }
    }
}
pub mod analysis;
mod lifecore;
pub mod typecheck;
