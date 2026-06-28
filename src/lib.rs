use indexmap::IndexSet;
use itertools::Itertools;
use jiff::{Timestamp, Zoned};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, hash::Hash};
// use thiserror;
use strum::{EnumIs, EnumTryAs};

use crate::analyzer::AnalysisSuite;

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Copy, Hash)]
pub struct SmallTable<K: Eq + Hash, V>([Option<(K, V)>; 32]);

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
pub struct CircumstanceIndex(u64);

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
pub struct StringIndex(u64);

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
pub struct EventIndex(u64);

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
    analyzer: AnalysisSuite
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq)]
pub enum LifeCoreError {}

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
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, strum::VariantNames)]
enum Payload {
    PreciseTime(Timestamp),
    Text(StringIndex),
    Number(i64),
}

// This is a convenient type for the user
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, strum::VariantNames)]
pub enum PayloadExt {
    PreciseTime(Timestamp),
    Text(String),
    Number(i64),
}

// Misc
impl<K: Eq + Hash, V> SmallTable<K, V> {
    const CAP: usize = 32;

    pub fn new() -> Self {
        Self([const { None }; Self::CAP])
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
        assert!(tab.len() <= Self::CAP, "HashMap too large!");
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

mod lifecore;
mod analyzer;
#[cfg(test)]
mod tests;
