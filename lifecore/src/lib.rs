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

use anyhow::Ok;
use indexmap::IndexSet;
use itertools::Itertools;
use jiff::{Timestamp, Zoned};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt::Display;
use std::ops::Index;
use std::{collections::HashMap, hash::Hash};
// use thiserror;
use strum::{EnumIs, EnumTryAs};

pub use crate::analyzer::AnalysisSuite;
pub use crate::graphviz::{GraphVizConfig, GraphVizConfigParentMode, graphviz};

// Used for "internal" representation of NodeBody
#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Copy, Hash)]
pub struct SmallTable<K: Eq + Hash, V>([Option<(K, V)>; 32]);

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

    // Analyzers added so far. An analyzer has 3 pieces of state:
    // - initial config
    // - nodes and edges it adds to the graph during initialization (`&mut LifeCore`)
    // - nodes and edges it encourages to add to the graph through AnalysisReport
    analyzer: AnalysisSuite,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub enum LifeCoreError {
    FieldDeserializationFailed { field_name: String },
    FieldNotFound { field_name: String },
    FieldContainsUnexpectedType { field_name: String },
}

pub enum AnalysisReportKind {
    Error,
    Inconsistent,
    Warning,
    Info,
}

pub struct AnalysisReport {
    kind: AnalysisReportKind,
    offending_event: EventIndex,
    message: String,
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
// NOTE at some point this became no longer quite "convenient" because serialization
// Now each variant has its own specific type as well. This is for the better because
// it allows node payload type definitions in Rust code.
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

// // Represents  a "non-trivial" JSON Value.
// // The problem is overeager serialization of primitives. Serde data-model serializes many things as the
// // underlying primitive type, and when deserializing we would have to try deserializing Strings and Values.
// #[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, strum::EnumIs, strum::EnumTryAs)]
// #[cfg_attr(debug_assertions, derive(Debug))]
// pub enum SerializedExt {
//     Array(Vec<serde_json::Value>),
//     Object(BTreeMap<String, serde_json::Value>),
// }

// // Misc

// impl TryFrom<&serde_json::Value> for SerializedExt {
//     type Error = LifeCoreError;

//     fn try_from(value: &serde_json::Value) -> Result<Self, Self::Error> {
//         match value {
//             serde_json::Value::Array(values) => {
//                 Result::<Self, Self::Error>::Ok(Self::Array(values.clone()))
//             }
//             serde_json::Value::Object(map) => Result::<Self, Self::Error>::Ok(Self::Object(
//                 map.clone()
//                     .into_iter()
//                     .collect::<BTreeMap<String, serde_json::Value>>(),
//             )),
//             // TODO better error
//             _ => Err(LifeCoreError::FieldDeserializationFailed {
//                 field_name: "TODO".to_string(),
//             }),
//         }
//     }
// }

// impl TryFrom<serde_json::Value> for SerializedExt {
//     type Error = LifeCoreError;

//     fn try_from(value: serde_json::Value) -> Result<Self, Self::Error> {
//         Self::try_from(&value)
//     }
// }

// impl Into<serde_json::Value> for SerializedExt {
//     fn into(self) -> serde_json::Value {
//         match self {
//             SerializedExt::Array(values) => serde_json::Value::Array(values.clone()),
//             SerializedExt::Object(btree_map) => {
//                 serde_json::Value::Object(serde_json::Map::from_iter(btree_map.clone().into_iter()))
//             }
//         }
//     }
// }

// impl Display for SerializedExt {
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         let v: serde_json::Value = self.clone().into();
//         v.fmt(f)
//     }
// }

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
    assert!(x.iter().map(|y| &y.0).counts().len() == x.len());
    for (k, v) in x.into_iter() {
        r.insert(k.clone(), v.clone());
    }
    r
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

// // Due to serde data model, sometimes a "non-primitive" type like enums or tuple structs get serialized
// // in a way that on deserialization all we get is a string. So, PayloadExt::Text also needs to be probed for
// // JSON data
// // TODO is there a scenario where this is undesirable? Could we be masking a deeper issue doing this?
// // In fact, the root cause is PayloadExt being untagged, but if it is tagged debugging is gonna be even harder
// // Maybe we should consider making PayloadExt serialize with serde defaults...

// impl PayloadExt {
//     pub fn deserialize<T: DeserializeOwned>(self) -> Option<T> {
//         match self {
//             PayloadExt::Text(t) => {
//                 if let Result::Ok(x) = serde_json::from_str::<T>(t.as_str()) {
//                     Some(x)
//                 } else {
//                     None
//                 }
//             }
//             PayloadExt::Serialized(serialized_ext) => {
//                 if let Result::Ok(x) = serde_json::from_value::<T>(serialized_ext.into()) {
//                     Some(x)
//                 } else {
//                     None
//                 }
//             }
//             _ => panic!("I don't know how to handle this deserialization scenario!"),
//         }
//     }
// }

// // Even though this is complete mess, at least this wrapper function will panic if you try to "serialize"
// // an offending type -> tests should include at least one instantiation and insertion of every node type we define
// impl PayloadExt {
//     // Would conflict with `impl<T> From<T> for T`
//     pub fn create<T: Serialize>(v: T) -> Self {
//         let json_attempt = serde_json::to_value(v).unwrap();
//         if let x = SerializedExt::try_from(&json_attempt)
//             && x.is_ok()
//         {
//             Self::Serialized(x.unwrap())
//         } else {
//             match json_attempt {
//                 serde_json::Value::Number(number) => {
//                     Self::Number(number.as_i64().expect("in-bounds number"))
//                 }
//                 serde_json::Value::String(s) => Self::Text(s),

//                 _ => panic!("Don't know how to handle serialization!"),
//             }
//         }
//     }
// }

impl NodeBody {
    pub fn get_number(&self, name: &str) -> Result<i64, LifeCoreError> {
        if !self.0.contains_key(name) {
            Err(LifeCoreError::FieldNotFound {
                field_name: name.to_string(),
            })
        } else {
            let x: Option<i64> = self["class"].clone().try_as_number();
            if let Some(y) = x {
                Result::<i64, LifeCoreError>::Ok(y)
            } else {
                Err(LifeCoreError::FieldContainsUnexpectedType {
                    field_name: name.to_string(),
                })
            }
        }
    }

    pub fn get_precise_time(&self, name: &str) -> Result<Timestamp, LifeCoreError> {
        if !self.0.contains_key(name) {
            Err(LifeCoreError::FieldNotFound {
                field_name: name.to_string(),
            })
        } else {
            let x: Option<Timestamp> = self["class"].clone().try_as_precise_time();
            if let Some(y) = x {
                Result::<Timestamp, LifeCoreError>::Ok(y)
            } else {
                Err(LifeCoreError::FieldContainsUnexpectedType {
                    field_name: name.to_string(),
                })
            }
        }
    }

    pub fn get_text(&self, name: &str) -> Result<String, LifeCoreError> {
        if !self.0.contains_key(name) {
            Err(LifeCoreError::FieldNotFound {
                field_name: name.to_string(),
            })
        } else {
            let x: Option<String> = self["class"].clone().try_as_text();
            if let Some(y) = x {
                Result::<String, LifeCoreError>::Ok(y)
            } else {
                Err(LifeCoreError::FieldContainsUnexpectedType {
                    field_name: name.to_string(),
                })
            }
        }
    }
    pub fn get_serialized<T: DeserializeOwned>(&self, name: &str) -> Result<T, LifeCoreError> {
        if !self.0.contains_key(name) {
            Err(LifeCoreError::FieldNotFound {
                field_name: name.to_string(),
            })
        } else {
            let x: Option<T> = self["class"].clone().deserialize();
            if let Some(y) = x {
                Result::<T, LifeCoreError>::Ok(y)
            } else {
                Err(LifeCoreError::FieldDeserializationFailed {
                    field_name: name.to_string(),
                })
            }
        }
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
