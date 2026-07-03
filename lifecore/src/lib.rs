//! Rumination Regarding Node Payload Field Types
//! =============================================
//!
//! A complete disaster. Initially I wanted to keep field types to a handful but that hit a wall immediately;
//! Everything became stringly-typed! It possible to remedy this by introducing a handful more of useful types
//! and constructing nodes only from those; however I've already hit a wall with Node_Type: should I add
//! collection types? Collection types containing what? It's a very brittle system.
//!
//! An alternative design would be having every node type register "itself" beforehand, with a validation function.
//! Ah.
//!
//! Because I value being able to know beforehand what a Node contains, we will go with a more straightforward approach
//! of explicitly declaring screwy data fields. i.e. Payload::Serialized

use indexmap::IndexSet;
use itertools::Itertools;
use jiff::{Timestamp, Zoned};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::fmt::Display;
use std::ops::Index;
use std::{collections::HashMap, hash::Hash};
use strum::{EnumIs, EnumTryAs};
use thiserror;

pub use crate::analyzer::AnalysisSuite;
pub use crate::graphviz::{GraphVizConfig, GraphVizConfigParentMode, graphviz};

// Used for "internal" representation of NodeBody
#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Copy, Hash)]
pub struct SmallTable<K: Eq + Hash, V>([Option<(K, V)>; 32]);

// Used for "external" convenient usage
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub struct NodeBody(HashMap<String, PayloadExt>);

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
pub struct CircumstanceIndex(u64);

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
pub struct StringIndex(u64);

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
pub struct EventIndex(u64);

impl EventIndex {
    pub fn before(&self) -> std::ops::Range<usize> {
        0..(self.0 as usize)
    }

    pub fn before_and_including(&self) -> std::ops::Range<usize> {
        // Can't use RangeInclusive because it can't be converted to Range easily
        0..((self.0 + 1) as usize)
    }

    pub fn between_inclusive(&self, other: Self) -> std::ops::Range<usize> {
        // Can't use RangeInclusive because it can't be converted to Range easily
        if self.0 > other.0 {
            (other.0 as usize)..((self.0 + 1) as usize)
        } else {
            (self.0 as usize)..((other.0 + 1) as usize)
        }
    }
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
    analyzer: AnalysisSuite,
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
}

#[derive(Serialize, Deserialize, EnumIs, EnumTryAs, Clone, PartialEq, Eq, Hash)]
pub enum Event {
    Node(Node),
    Edge(Edge),
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Node {
    ty: StringIndex,
    parent: Option<EventIndex>,
    payload: SmallTable<StringIndex, Payload>,
}
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Edge {
    ty: StringIndex,
    from: EventIndex,
    to: EventIndex,
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
pub enum Payload {
    PreciseTime(Timestamp),
    Text(StringIndex),
    Number(i64),
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
    Serialized(serde_json::Value),
}

impl PayloadExt {
    pub fn serialized_from<T: Serialize>(x: T) -> Self {
        Self::Serialized(serde_json::to_value(x).unwrap())
    }
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum AnalysisReport {
    Warning(AnalysisReportWarning),
    Suggestion(AnalysisReportSuggestion),
    LogInfo(AnalysisReportLog),
    Inconsistent(AnalysisReportInconsistent),
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum AnalysisReportWarning {
    Deprecation {
        is_node: bool,
        e: EventIndex,
        field: Option<String>,
    },
}
#[derive(Serialize, Deserialize, PartialEq, Eq, Clone)]

pub enum AnalysisReportSuggestion {
    AddNode { ty: String, field: NodeBody },
}
#[derive(Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum AnalysisReportInconsistent {
    BasicValidationFailure {
        offending_event: EventIndex,
        reason: BasicValidationFailureScenario,
    },
}
#[derive(Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum BasicValidationFailureScenario {}

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone)]
pub enum AnalysisReportLog {}

// Misc/Boilerplate

impl<K: Eq + Hash, V> SmallTable<K, V> {
    pub fn new() -> Self {
        Self([const { None }; 32])
    }

    pub fn insert(&mut self, key: K, val: V) {
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

    pub fn from(tab: HashMap<K, V>) -> Self {
        assert!(tab.len() <= 32, "HashMap too large!");
        let mut r = Self::new();
        for x in tab.into_iter() {
            r.insert(x.0, x.1);
        }
        return r;
    }
}

pub fn to_map<K: Eq + Hash + Clone, V: Clone>(x: &[(K, V)]) -> HashMap<K, V> {
    let mut r = HashMap::<K, V>::new();
    assert!(
        x.iter().map(|y| &y.0).counts().len() == x.len(),
        "Supplied list has a repeated key (not a proper set!)"
    );
    for (k, v) in x.into_iter() {
        r.insert(k.clone(), v.clone());
    }
    r
}

// NOTE one helper method per Payload type
impl NodeBody {
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

impl Index<&str> for NodeBody {
    type Output = PayloadExt;

    fn index(&self, index: &str) -> &Self::Output {
        self.0.index(index)
    }
}

pub(crate) mod analyzer;
mod graphviz;
mod lifecore;
#[cfg(test)]
mod tests;
