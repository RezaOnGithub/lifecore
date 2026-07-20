//! Reference Implementation of LifeCore Using Simple In-memory Lists
//! =================================================================
//!
//! This implementation of [`crate::LifeCore`] trait is focused on correctness
//! and simplicity; performance concerns are out of scope for now.
//!
//! [`LifeCoreList`] stores every [`crate::Payload`] value with at least one
//! indirection. (Yes; even for numerical values, which is not ideal) This
//! indirection sometimes requires recursive look-ups to resolve a value, which
//! is not at all ideal even for the hypothetical SQLite-based port- least of
//! which due to complexity in querying.

use crate::{
    BasicValidationFailure,
    BasicValidationFailureScenario::{self, *},
    Circumstance, Context, Entry, EntryContext, EntryIndex, LifeCore, LifeCoreError, Payload,
    Timestamp,
};
use indexmap::IndexSet;
use itertools::Itertools;
use jiff::{Unit, Zoned};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, str::FromStr};
use strum::VariantNames;

// TODO rename all X-prefixed names to more verbose names

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
struct ValueIndex(usize);

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
struct AggregateIndex(usize);

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
struct StringIndex(usize);

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
struct CircumstanceIndex(usize);

/// *Note:* This is not `Copy` because quite a number of useful types in `jiff`
/// are not `Copy`
#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, strum::VariantNames, Hash)]
enum Value {
    PreciseTime(Timestamp),
    Text(StringIndex),
    Number(i64),
    Aggregate(AggregateIndex),
    Serialized(StringIndex),
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, strum::EnumTryAs, strum::EnumIs, Hash)]
enum XEntry {
    Node(XNode),
    Edge(XEdge),
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Hash)]
struct XNode {
    ty: StringIndex,
    parent: Option<EntryIndex>,
    body: BTreeMap<StringIndex, ValueIndex>,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Copy, Hash)]
struct XEdge {
    ty: StringIndex,
    from: EntryIndex,
    to: EntryIndex,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct LifeCoreList {
    entry: Vec<XEntry>,
    circumstance: Vec<CircumstanceIndex>,

    // Layers of Indirection
    circumstance_table: IndexSet<Circumstance>,
    string_table: IndexSet<String>,
    aggregate_table: IndexSet<Vec<ValueIndex>>,
    value_table: IndexSet<Value>,
}

impl LifeCoreList {
    fn get_or_insert_value(&mut self, v: &Value) -> ValueIndex {
        if let Some(i) = self.value_table.get_index_of(v).map(|x| ValueIndex(x)) {
            i
        } else {
            assert!(
                self.value_table.insert(v.clone()),
                "The value should not be present; we just checked!"
            );
            ValueIndex(self.value_table.len() - 1)
        }
    }

    fn resolve_value(&self, i: ValueIndex) -> Payload {
        match self.value_table[i.0] {
        Value::PreciseTime(timestamp) => Payload::PreciseTime(timestamp),
        Value::Text(string_index) => Payload::Text(self.resolve_string(string_index)),
        Value::Number(x) => Payload::Number(x),
        Value::Aggregate(aggregate_index) => {
            Payload::Aggregate(self.resolve_aggregate(aggregate_index))
        }
        Value::Serialized(string_index) => {
            Payload::Serialized(crate::JSON::from_str(&self.resolve_string(string_index)).unwrap())
        }
        }
    }

    // COPY_PASTE
    fn get_or_insert_string(&mut self, v: &str) -> StringIndex {
        if let Some(i) = self.string_table.get_index_of(v).map(|x| StringIndex(x)) {
            i
        } else {
            assert!(
                self.string_table.insert(v.to_string()),
                "The value should not be present; we just checked!"
            );
            StringIndex(self.string_table.len() - 1)
        }
    }

    fn resolve_string(&self, i: StringIndex) -> String {
        self.string_table[i.0].to_string()
    }

    // COPY_PASTE
    fn get_or_insert_aggregate(&mut self, v: &Vec<ValueIndex>) -> AggregateIndex {
        if let Some(i) = self
            .aggregate_table
            .get_index_of(v)
            .map(|x| AggregateIndex(x))
        {
            i
        } else {
            assert!(
                self.aggregate_table.insert(v.clone()),
                "The value should not be present; we just checked!"
            );
            AggregateIndex(self.aggregate_table.len() - 1)
        }
    }

    fn resolve_aggregate(&self, i: AggregateIndex) -> Vec<Payload> {
        self.aggregate_table[i.0]
            .iter()
            .map(|x| self.resolve_value(*x))
            .collect()
    }

    // COPY_PASTE
    fn get_or_insert_circumstance(&mut self, c: &Circumstance) -> CircumstanceIndex {
        if let Some(i) = self
            .circumstance_table
            .get_index_of(c)
            .map(|x| CircumstanceIndex(x))
        {
            i
        } else {
            assert!(
                self.circumstance_table.insert(c.clone()),
                "The value should not be present; we just checked!"
            );
            CircumstanceIndex(self.circumstance_table.len() - 1)
        }
    }

    fn resolve_circumstance(&self, i: CircumstanceIndex) -> Circumstance {
        self.circumstance_table[i.0].clone()
    }

    fn insert_payload(&mut self, v: &Payload) -> ValueIndex {
        match v {
        Payload::PreciseTime(timestamp) => {
            self.get_or_insert_value(&Value::PreciseTime(*timestamp))
        }
        Payload::Text(s) => {
            let string_index = self.get_or_insert_string(s);
            self.get_or_insert_value(&Value::Text(string_index))
        }
        Payload::Number(n) => self.get_or_insert_value(&Value::Number(*n)),
        Payload::Aggregate(payloads) => {
            let value_indices = payloads
                .iter()
                .map(|x| self.insert_payload(x))
                .collect_vec();
            let aggregate_index = self.get_or_insert_aggregate(&value_indices);
            self.get_or_insert_value(&Value::Aggregate(aggregate_index))
        }
        Payload::Serialized(v) => {
            let json_string = v.to_string();
            let string_index = self.get_or_insert_string(&json_string);
            self.get_or_insert_value(&Value::Text(string_index))
        }
        }
    }

    fn payload_exists(&self, v: &Payload) -> bool {
        // Contrary to what `rust-analyzer` says, `mut` is very much necessary here
        // TODO open an issue?
        let mut clone = self.clone();
        let value_index = clone.insert_payload(v);
        self.value_table.len() > value_index.0
    }

    fn raw_window<I>(&self, i: I) -> impl Iterator<Item = XEntry>
    where
        I: IntoIterator<Item = EntryContext>,
    {
        i.into_iter()
            .map_while(|x| self.entry.get(x.thing).cloned())
    }

    fn raw_edges(&self, ctx: Context) -> impl Iterator<Item = (EntryContext, XEdge)> {
        let w = || ctx.items();
        w().zip(self.raw_window(w()))
            .filter_map(|(a, b)| b.try_as_edge().map(|x| (a, x)))
    }

    fn raw_nodes(&self, ctx: Context) -> impl Iterator<Item = (EntryContext, XNode)> {
        let w = || ctx.items();
        w().zip(self.raw_window(w()))
            .filter_map(|(a, b)| b.try_as_node().map(|x| (a, x)))
    }
}

impl LifeCore for LifeCoreList {
    fn last(&self) -> Option<crate::EntryContext> {
        if self.entry.len() == 0 {
            None
        } else {
            Some(crate::EntryContext {
                thing: self.entry.len() - 1,
                end: self.entry.len(),
            })
        }
    }

    fn total_event_count(&self) -> usize {
        self.entry.len()
    }

    fn basic_validation(&self) -> Result<(), crate::BasicValidationFailure> {
        let ctx = || Context::universe();

        // Non-Rules. Have little to do with the meaning of the graph, but more related
        // to data integrity
        {
            // Payload and PayloadExt cover the same types
            {
                // TTODO This should be a compile-time check instead, somehow
                assert!(Payload::VARIANTS.to_vec() == Value::VARIANTS.to_vec())
            }

            // Any String in the StringTable is referenced at least once somewhere
            {
                // TODO
            }

            // Any Circumstance in CircumstanceTable is referenced at least once
            // ... and not repeated
            {
                // TODO
            }

            // TODO and so on for every value type, recursively; We do shouldn't allow
            // residual values that are unused

            // Every Event happens under a Circumstance
            {
                if self.entry.len() != self.circumstance.len() {
                    return Err(BasicValidationFailure {
                        offending_event: ctx().into(),
                        reason: InternalError {
                            message: "Circumstance list does not match up with Event list"
                                .to_string(),
                        },
                    });
                }
            }

            // Events under Circumstance::Mock must form a contiguous suffix. This is
            // used for validating provisional changes before applying them.
            {
                if self.circumstance_table.contains(&Circumstance::Mock) {
                    let mock_index = self
                        .circumstance_table
                        .get_index_of(&Circumstance::Mock)
                        .unwrap();
                    let mock_event_index = self
                        .circumstance
                        .iter()
                        .enumerate()
                        .filter(|x| x.1.0 == mock_index)
                        .map(|x| x.0)
                        .collect_vec();
                    assert!(!mock_event_index.is_empty());
                    if !mock_event_index
                        .iter()
                        .copied()
                        .eq(mock_event_index[0]..self.entry.len())
                    {
                        // TODO better context
                        return Err(BasicValidationFailure {
                            offending_event: ctx().into(),
                            reason: ExtraneousMockCircumstance,
                        });
                    }
                }
            }
        }

        // Rule: exact edge duplicate events are forbidden.
        // Rule: exact node duplicate events are forbidden if both are valid within a
        // neighborhood; node duplicates are _only discouraged_ otherwise
        {
            // Easy part: check edges
            let edge_count = self.edges(ctx()).count();
            let unique_edge_count = self.raw_edges(ctx()).map(|x| x.1).unique().count();
            if edge_count != unique_edge_count {
                // TODO not so easy now; give better context
                return Err(BasicValidationFailure {
                    offending_event: ctx().into(),
                    reason: DuplicateEdge,
                });
            }

            // Hard part: check nodes
            let dupe_class = self
                .raw_nodes(ctx())
                .into_group_map_by(|x| x.1.clone())
                .into_values()
                .filter(|bucket| bucket.len() > 1)
                .collect_vec();

            for dupe in dupe_class {
                for pair in dupe.iter().combinations(2) {
                    let ((older_index, _older_node), (newer_index, newer_node)) = {
                        if pair[0].0.thing < pair[1].0.thing {
                            (pair[0], pair[1])
                        } else {
                            (pair[1], pair[0])
                        }
                    };

                    if self
                        .is_stale(newer_index.shrink_to_exclusive_left().look_at(*older_index))
                        .unwrap()
                    {
                        // TODO move this Warning into its own analyzer
                        // warn!(
                        //     *newer_index,
                        //     "Duplicate nodes, but older one was invalidated
                        // before the new one was added (discouraged).
                        // EntryIndex {} and {}",
                        //     older_index,
                        //     newer_index
                        // );
                    } else {
                        assert!({
                            // The check above should pass if an identical node overrides the parent
                            // What is the application of overriding a node with an exact copy?
                            // (Maybe counters, or means of correcting edges or getting rid of stale
                            // edges)
                            if let Some(idx) = newer_node.parent
                                && idx == older_index.entry()
                            {
                                false
                            } else {
                                true
                            }
                        });
                        return Err(BasicValidationFailure {
                            offending_event: (*newer_index).into(),
                            reason: DuplicateNode,
                        });
                    }
                }
            }
        }

        // Rule: events are sorted, nodes and edges cannot refer to themselves or the
        // future Rule: Edges can only be between nodes
        // Rule: Node parent can only be another node

        for e in self.entry.iter().enumerate().map(|(a,b)| (Into::<EntryIndex>::into(a), b)) {
            let idx_in_range = |idx: &EntryIndex| (idx.0) < e.0.0;
            let offending_event = ctx().look_at(e.0.before_and_including());
            match e.1 {
            XEntry::Node(XNode {
                ty: _,
                parent,
                body: _,
            }) => {
                if let Some(idx) = parent {
                    if !idx_in_range(idx) {
                        return Err(BasicValidationFailure {
                            offending_event: offending_event.into(),
                            reason: OutOfRangeParent,
                        });
                    }
                    if self.entry[idx.0].is_edge() {
                        return Err(BasicValidationFailure {
                            offending_event: offending_event.into(),
                            reason: EdgeParent,
                        });
                    }
                }
            }
            Event::Edge(Edge { ty: _, from, to }) => {
                let idx = EntryIndex::from(e.0);
                if !idx_in_range(from) || !idx_in_range(to) {
                    return Err(BasicValidationFailure {
                        offending_event: ctx().look_at(idx.before_and_including()).into(),
                        reason: OutOfRangeEdgeEnd,
                    });
                }
                if self[from].is_edge() || self[to].is_edge() {
                    return Err(BasicValidationFailure {
                        offending_event: ctx().look_at(idx.before_and_including()).into(),
                        reason: EdgeEndIsEdge,
                    });
                }
            }
            }
        }

        // Rule: after a node has been overriden (that is, a new "child" node has been
        // added) it cannot be a part of a new edge. "Stale Rule"
        for (edge_context, edge) in self.raw_edges(ctx()) {
            // Throw away the loop context. Now we are looking at 0..edge_index
            // (inclusive or exclusive doesn't matter in this particular case)
            let ctx = edge_context.shrink_to_exclusive_left();
            assert!(edge_context.thing.0 > edge.from.0 && edge_context.thing.0 > edge.to.0);
            if self
                .is_stale(ctx.look_at(edge.from.before_and_including()))
                .unwrap()
                || self
                    .is_stale(ctx.look_at(edge.to.before_and_including()))
                    .unwrap()
            {
                return Err(BasicValidationFailure {
                    offending_event: edge_context
                        .shrink_to_inclusive_left()
                        .last()
                        .unwrap()
                        .into(),
                    reason: StaleRuleViolation,
                });
            }
        }

        Ok(())
    }

    fn connect_mock(
        &mut self,
        from: crate::EntryContext,
        ty: &str,
        to: crate::EntryContext,
    ) -> Result<crate::Edge, crate::LifeCoreError> {
        todo!()
    }

    fn add_mock(
        &mut self,
        ty: &str,
        payload: crate::Body,
        parent: Option<crate::EntryContext>,
    ) -> Result<crate::Node, crate::LifeCoreError> {
        todo!()
    }

    fn is_mock(&self) -> bool {
        todo!()
    }

    fn apply(&mut self) -> Result<(), crate::LifeCoreError> {
        todo!()
    }

    fn get(&self, e: crate::EntryContext) -> Option<crate::Entry> {
        todo!()
    }

    fn get_with_circumstance(
        &self,
        e: crate::EntryContext,
    ) -> Option<(crate::Entry, Circumstance)> {
        todo!()
    }

    fn window<I>(&self, i: I) -> impl Iterator<Item = crate::Entry>
    where
        I: IntoIterator<Item = crate::EntryContext>,
    {
        todo!()
    }

    fn is_stale(&self, e: crate::EntryContext) -> Option<bool> {
        todo!()
    }

    fn query_pick_node(&self, query: Vec<crate::NodeQueryPredicate>) -> Vec<crate::Node> {
        todo!()
    }

    fn query_filter_node(&self, query: Vec<crate::NodeQueryPredicate>) -> Vec<crate::Node> {
        todo!()
    }

    fn node(&self, ctx: crate::EntryContext) -> Option<crate::Node> {
        todo!()
    }

    fn edge(&self, ctx: crate::EntryContext) -> Option<crate::Edge> {
        todo!()
    }

    fn nodes(&self, ctx: crate::Context) -> impl Iterator<Item = crate::Node> {
        todo!()
    }

    fn edges(&self, ctx: crate::Context) -> impl Iterator<Item = crate::Edge> {
        todo!()
    }

    fn root_of(&self, e: crate::EntryContext) -> Option<crate::EntryContext> {
        todo!()
    }

    fn fresh_successors(&self, node: crate::EntryContext) -> Vec<crate::EntryContext> {
        todo!()
    }

    fn relative_degree(&self, a: crate::EntryContext, b: crate::EntryContext) -> Option<usize> {
        todo!()
    }

    fn families(&self, ctx: crate::Context) -> Vec<crate::LifeCoreFamily> {
        todo!()
    }

    fn add_analyzer(&mut self, name: &str, config: &str) {
        todo!()
    }

    fn current_analysis_suite(&self) -> crate::analysis::SuiteConfig {
        todo!()
    }
}
