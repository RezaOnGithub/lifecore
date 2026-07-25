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
    BasicValidationFailure, BasicValidationFailureScenario::*, Circumstance, Context, EntryContext,
    EntryIndex, FamilyInsert, LifeCore, LifeCoreError, LifeCoreFamily, NodeQuery,
    NodeQueryPredicate, OrderedSet, Payload, Timestamp, analysis,
};
use indexmap::IndexSet;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use strum::{IntoDiscriminant, VariantNames};

// TODO rename all X-prefixed names to more verbose names

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
struct ValueIndex(usize);

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
struct SeqeunceIndex(usize);

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
struct OrderedSetIndex(usize);

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
    Sequence(SeqeunceIndex),
    OrderedSet(OrderedSetIndex),
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

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct LifeCoreList {
    entry: Vec<XEntry>,
    circumstance: Vec<CircumstanceIndex>,
    suite: analysis::SuiteConfig,

    // Layers of Indirection
    circumstance_table: IndexSet<Circumstance>,
    string_table: IndexSet<String>,
    sequence_table: IndexSet<Vec<ValueIndex>>,
    ordered_set_table: IndexSet<OrderedSet<ValueIndex>>,
    value_table: IndexSet<Value>,
}

impl LifeCoreList {
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

    // NOTE take care not to call this on the actual database! First, invoke on a
    // clone; if successful, write clone into &mut self elsewhere.
    fn add_and_validate(
        &mut self,
        ty: &str,
        body: crate::Body,
        parent: Option<crate::EntryContext>,
    ) -> Result<(), crate::LifeCoreError> {
        let circumstance = self.get_or_insert_circumstance(&Circumstance::Mock);
        let ty = self.get_or_insert_string(ty);
        let mut internal_body = BTreeMap::<StringIndex, ValueIndex>::new();
        for (name, payload) in body.0.iter() {
            let name = self.get_or_insert_string(name);
            let internal_value_index = self.insert_payload(payload);
            internal_body.insert(name, internal_value_index);
        }
        self.entry.push(XEntry::Node(XNode {
            ty,
            parent: parent.map(|x| x.entry()),
            body: internal_body,
        }));
        self.circumstance.push(circumstance);
        self.basic_validation()?;
        Ok(())
    }
    // NOTE take care not to call this on the actual database! First, invoke on a
    // clone; if successful, write clone into &mut self elsewhere.
    fn connect_and_validate(
        &mut self,
        from: crate::EntryContext,
        ty: &str,
        to: crate::EntryContext,
    ) -> Result<(), LifeCoreError> {
        let ty = self.get_or_insert_string(ty);
        let circumstance = self.get_or_insert_circumstance(&Circumstance::Mock);
        self.entry.push(XEntry::Edge(XEdge {
            ty,
            from: from.entry(),
            to: to.entry(),
        }));
        self.circumstance.push(circumstance);
        self.basic_validation()?;
        Ok(())
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

    fn add_analyzer(&mut self, name: &str, config: &str) {
        self.suite.0.push(analysis::Config {
            name: name.to_string(),
            config: config.to_string(),
        });
    }

    fn current_analysis_suite(&self) -> crate::analysis::SuiteConfig {
        self.suite.clone()
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
                        .filter(|(_, CircumstanceIndex(i))| *i == mock_index)
                        .map(|(index, _)| index)
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

        for e in self
            .entry
            .iter()
            .enumerate()
            .map(|(a, b)| (Into::<EntryIndex>::into(a), b))
        {
            let idx_in_range = |idx: &EntryIndex| *idx < e.0;
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
            XEntry::Edge(XEdge { ty: _, from, to }) => {
                let idx = e.0;
                if !idx_in_range(from) || !idx_in_range(to) {
                    return Err(BasicValidationFailure {
                        offending_event: ctx().look_at(idx.before_and_including()).into(),
                        reason: OutOfRangeEdgeEnd,
                    });
                }
                if self.entry[from.0].is_edge() || self.entry[to.0].is_edge() {
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
            assert!(edge_context.entry() > edge.from && edge_context.entry() > edge.to.into());
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
        let mut clone = self.clone();
        clone.connect_and_validate(from, ty, to)?;
        *self = clone;
        Ok(self.edge(self.last().unwrap()).unwrap())
    }

    fn add_mock(
        &mut self,
        ty: &str,
        body: crate::Body,
        parent: Option<crate::EntryContext>,
    ) -> Result<crate::Node, crate::LifeCoreError> {
        let mut clone = self.clone();
        clone.add_and_validate(ty, body, parent)?;
        *self = clone;
        Ok(self.node(self.last().unwrap()).unwrap())
    }

    fn is_mock(&self) -> bool {
        self.circumstance_table.contains(&Circumstance::Mock)
    }

    // SUS have a second look at "current circumstance actually pre-existed" case
    fn apply(&mut self) -> Result<(), crate::LifeCoreError> {
        if !self.is_mock() {
            return Ok(());
        }

        let mut candidate = self.clone();
        candidate.basic_validation()?;
        let mock_index = CircumstanceIndex(
            candidate
                .circumstance_table
                .get_index_of(&Circumstance::Mock)
                .unwrap(),
        );

        assert!(
            matches!(
                candidate.circumstance_table.last(),
                Some(Circumstance::Mock)
            ),
            "Mock must be the final circumstance"
        );
        assert!(
            matches!(candidate.circumstance_table.pop(), Some(Circumstance::Mock)),
            "The final circumstance must be Mock"
        );

        let current_circumstance_index =
            candidate.get_or_insert_circumstance(&Circumstance::current_session()?);
        if current_circumstance_index != mock_index {
            for circumstance in candidate.circumstance.iter_mut() {
                if *circumstance == mock_index {
                    *circumstance = current_circumstance_index;
                }
            }
        }

        candidate.basic_validation()?;
        *self = candidate;
        Ok(())
    }

    fn get(&self, e: crate::EntryContext) -> Option<crate::Entry> {
        let x = self.entry.get(e.thing);
        x.map(|x| match x {
        XEntry::Node(xnode) => crate::Entry::Node(crate::Node {
            e,
            ty_name: self[xnode.ty].to_string(),
            field: crate::Body(
                xnode
                    .body
                    .iter()
                    .map(|(k, v)| (self[*k].to_string(), self.get_payload(v)))
                    .collect(),
            ),
            parent: xnode.parent,
        }),
        XEntry::Edge(xedge) => crate::Entry::Edge(crate::Edge {
            e,
            ty_name: self[xedge.ty].to_string(),
            from: xedge.from,
            to: xedge.to,
        }),
        })
    }

    fn get_with_circumstance(
        &self,
        e: crate::EntryContext,
    ) -> Option<(crate::Entry, Circumstance)> {
        let circumstance = *self.circumstance.get(e.thing)?;
        self.get(e).map(|entry| (entry, self[circumstance].clone()))
    }

    fn window<I>(&self, i: I) -> impl Iterator<Item = crate::Entry>
    where
        I: IntoIterator<Item = crate::EntryContext>,
    {
        i.into_iter().map_while(|x| self.get(x))
    }

    fn is_stale(&self, e: crate::EntryContext) -> Option<bool> {
        if e.thing >= self.entry.len() {
            return None;
        }
        match &self.raw(e.entry()) {
        XEntry::Node(_) => {
            let ctx = || e.inclusive_right_items();
            for event in self.raw_window(ctx()) {
                if event
                    .try_as_node()
                    .map(|node| {
                        node.parent
                            .is_some_and(|parent| parent.absolute() == e.absolute())
                    })
                    .is_some_and(|x| x)
                {
                    return Some(true);
                }
            }
        }
        XEntry::Edge(XEdge { ty: _, from, to }) => {
            // SUS
            // TODO use is_some but it also has to be false
            if self.is_stale(e.look_at(from.absolute())).unwrap()
                || self.is_stale(e.look_at(to.absolute())).unwrap()
            {
                return Some(true);
            }
        }
        }
        Some(false)
    }

    // TODO don't resolve every value for every single entry!
    fn nodes(&self, ctx: crate::Context) -> impl Iterator<Item = crate::Node> {
        ctx.into_iter()
            .map_while(move |x| self.get(ctx.look_at(EntryIndex(x).before_and_including())))
            .filter_map(|x| x.try_as_node())
    }

    // COPY_PASTE
    fn edges(&self, ctx: crate::Context) -> impl Iterator<Item = crate::Edge> {
        ctx.into_iter()
            .map_while(move |x| self.get(ctx.look_at(EntryIndex(x).before_and_including())))
            .filter_map(|x| x.try_as_edge())
    }

    fn root_of(&self, e: crate::EntryContext) -> Option<crate::EntryContext> {
        let index = e.entry();
        if self.raw(index).is_edge() {
            return None;
        }
        let mut r = index;
        while let Some(parent_event) = self.raw(r).clone().try_as_node().unwrap().parent {
            r = parent_event;
        }
        Some(e.look_at(r.absolute()))
    }

    fn fresh_successors(&self, node: crate::EntryContext) -> Vec<crate::EntryContext> {
        if self.get(node).is_none_or(|x| x.is_edge()) {
            return vec![];
        }
        let w = || node.exclusive_right_items();
        let mut successors = BTreeSet::<EntryContext>::from([node.absolute()]);
        let mut roots = BTreeSet::<EntryContext>::new();
        for (candidate_index, candidate) in w()
            .zip(self.raw_window(w()))
            .filter(|(_, event)| event.is_node())
            .map(|(i, e)| (i, e.try_as_node().unwrap()))
        {
            assert!(successors.intersection(&roots).nth(0).is_none());
            assert!(successors.iter().all(|x| *x == x.absolute()));
            assert!(roots.iter().all(|x| *x == x.absolute()));
            if let Some(candidate_parent) = candidate.parent {
                let candidate_parent: EntryContext = candidate_parent.absolute();
                if successors.contains(&candidate_parent) {
                    successors.remove(&candidate_parent);
                    roots.insert(candidate_parent);
                    successors.insert(candidate_index.absolute());
                } else if roots.contains(&candidate_parent) {
                    successors.insert(candidate_index.absolute());
                }
            }
        }
        successors.into_iter().map(|x| node.look_at(x)).collect()
    }

    fn relative_degree(&self, a: crate::EntryContext, b: crate::EntryContext) -> Option<usize> {
        let ancestor_chain = |mut n: EntryContext| {
            let mut chain = vec![];
            while let Some(parent) = self.node(n).unwrap().parent {
                let parent = n.look_at(parent.absolute());
                chain.push(parent.absolute());
                n = parent;
            }
            chain
        };

        if !self.get(a).is_some_and(|x| x.is_node()) || !self.get(b).is_some_and(|x| x.is_node()) {
            return None;
        }

        let left_ancestors = ancestor_chain(a);
        let right_ancestors = ancestor_chain(b);
        left_ancestors
            .iter()
            .enumerate()
            .flat_map(|(left_degree, left_ancestor)| {
                right_ancestors
                    .iter()
                    .enumerate()
                    .filter(move |(_, right_ancestor)| *right_ancestor == left_ancestor)
                    .map(move |(right_degree, _)| left_degree.max(right_degree))
            })
            .min()
    }

    fn families(&self, ctx: crate::Context) -> Vec<crate::LifeCoreFamily> {
        let mut r = Vec::<LifeCoreFamily>::new();
        for (index, node) in self.raw_nodes(ctx) {
            if node.parent.is_none_or(|p| !ctx.range().contains(&p.0)) {
                // We consider this a root node
                r.push(LifeCoreFamily::new_with_root(index));
            } else {
                // This must be a child node
                assert!(index == ctx.look_at(index.absolute()));
                r.iter_mut().for_each(|x| {
                    x.insert(FamilyInsert {
                        parent: ctx.look_at(node.parent.unwrap().absolute()),
                        child: index,
                    })
                });
            }
        }
        r
    }

    fn query(&self, query: NodeQuery) -> Vec<crate::Node> {
        self.raw_nodes(query.context)
            .filter(|(ctx, _)| {
                let combinator = query.expression.discriminant();
                combinator.combine(
                    query
                        .expression
                        .predicates()
                        .iter()
                        .map(|pred| pred.test(self, ctx.entry(), ctx.window())),
                )
            })
            .map(|(ctx, _)| self.node(ctx).unwrap())
            .collect()
    }

    fn explain_query(
        &self,
        query: &NodeQuery,
        candidate: EntryContext,
    ) -> Option<crate::NodeQueryExplanation> {
        let candidate = self
            .raw_nodes(query.context)
            .find(|(context, _)| context.entry() == candidate.entry())?
            .0;
        let combinator = query.expression.discriminant();
        let predicate_matches = query
            .expression
            .predicates()
            .iter()
            .map(|predicate| predicate.test(self, candidate.entry(), candidate.window()))
            .collect::<Vec<_>>();
        let matches = combinator.combine(predicate_matches.iter().copied());

        Some(crate::NodeQueryExplanation {
            predicate_matches,
            matches,
        })
    }
}

impl NodeQueryPredicate {
    fn test(&self, lc: &LifeCoreList, candidate_index: EntryIndex, query_context: Context) -> bool {
        let Some(_) = lc.raw(candidate_index).clone().try_as_node() else {
            eprintln!("Somehow, an edge index was passed to a node query");
            return false;
        };
        use NodeQueryPredicate::*;
        match self {
        FieldExists(query_field_path) => {
            let x = lc.node(candidate_index.absolute()).unwrap();
            let Some(field_content) = x.field.0.get(&query_field_path.field_name).cloned() else {
                return false;
            };
            !field_content.get(&query_field_path.path).is_empty()
        }

        FieldKind(query_field_path, payload_kind) => {
            let x = lc.node(candidate_index.absolute()).unwrap();
            let Some(field_content) = x.field.0.get(&query_field_path.field_name).cloned() else {
                return false;
            };
            // SUS the conjunction here! I think it would make sense for it to be here but
            // I'm not quite sure.

            // TO_TEST find a scenario where `.is_some_and` would cause unexpected output
            field_content
                .get(&query_field_path.path)
                .iter()
                .any(|x| x.discriminant() == *payload_kind)
        }

        FieldEqual(query_field_path, payload) => {
            let x = lc.node(candidate_index.absolute()).unwrap();
            let Some(field_content) = x.field.0.get(&query_field_path.field_name).cloned() else {
                return false;
            };
            field_content
                .get(&query_field_path.path)
                .iter()
                .any(|x| *x == *payload)
        }

        FieldIsStringAndMatches(query_field_path, query_regex) => {
            let x = lc.node(candidate_index.absolute()).unwrap();
            let Some(field_content) = x.field.0.get(&query_field_path.field_name).cloned() else {
                return false;
            };
            field_content.get(&query_field_path.path).iter().any(|x| {
                x.clone()
                    .try_as_text()
                    .is_some_and(|text| query_regex.is_match(text.as_str()))
            })
        }

        FieldIsSequenceAndContains(query_field_path, payload) => {
            let x = lc.node(candidate_index.absolute()).unwrap();
            let Some(field_content) = x.field.0.get(&query_field_path.field_name).cloned() else {
                return false;
            };
            field_content.get(&query_field_path.path).iter().any(|x| {
                x.clone()
                    .try_as_sequence()
                    .is_some_and(|aggregate| aggregate.contains(payload))
            })
        }

        FieldIsSequenceAndElementCountIs(query_field_path, count) => {
            let x = lc.node(candidate_index.absolute()).unwrap();
            let Some(field_content) = x.field.0.get(&query_field_path.field_name).cloned() else {
                return false;
            };
            field_content.get(&query_field_path.path).iter().any(|x| {
                x.clone()
                    .try_as_sequence()
                    .is_some_and(|aggregate| aggregate.len() == *count)
            })
        }

        FieldIsOrderedSetAndContains(field_query, payload) => {
            let x = lc.node(candidate_index.absolute()).unwrap();
            let Some(field_content) = x.field.0.get(&field_query.field_name).cloned() else {
                return false;
            };
            field_content.get(&field_query.path).iter().any(|x| {
                x.clone()
                    .try_as_ordered_set()
                    .is_some_and(|set| set.0.contains(payload))
            })
        }
        FieldIsOrderedSetAndElementCountIs(field_query, count) => {
            let x = lc.node(candidate_index.absolute()).unwrap();
            let Some(field_content) = x.field.0.get(&field_query.field_name).cloned() else {
                return false;
            };
            field_content.get(&field_query.path).iter().any(|x| {
                x.clone()
                    .try_as_ordered_set()
                    .is_some_and(|set| set.0.len() == *count)
            })
        }

        FieldQueryResultCount(field_query, count) => {
            let x = lc.node(candidate_index.absolute()).unwrap();
            let Some(field_content) = x.field.0.get(&field_query.field_name).cloned() else {
                return false;
            };
            field_content.get(&field_query.path).iter().count() == *count
        }

        Fresh => !lc
            .is_stale(query_context.look_at(candidate_index.absolute()))
            .unwrap(),
        NodeType(node_ty_name) => lc
            .string_table
            .get_index_of(node_ty_name)
            .map_or(false, |index| {
                index == lc.raw(candidate_index).clone().try_as_node().unwrap().ty.0
            }),
        HasOutgoingEdgeOfType(edge_ty_name) => {
            lc.string_table
                .get_index_of(edge_ty_name)
                .map_or(false, |edge_ty_name_index| {
                    lc.raw_edges(query_context).any(|(edge_index, xedge)| {
                        xedge.from == candidate_index && xedge.ty.0 == edge_ty_name_index
                    })
                })
        }

        // COPY_PASTE
        HasIncomingEdgeOfType(edge_ty_name) => {
            lc.string_table
                .get_index_of(edge_ty_name)
                .map_or(false, |edge_ty_name_index| {
                    lc.raw_edges(query_context).any(|(edge_index, xedge)| {
                        xedge.to == candidate_index && xedge.ty.0 == edge_ty_name_index
                    })
                })
        }

        HasFreshOutgoingEdgeOfType(edge_ty_name) => lc
            .string_table
            .get_index_of(edge_ty_name)
            .map_or(false, |edge_ty_name_index| {
                lc.raw_edges(query_context).any(|(edge_index, xedge)| {
                    xedge.from == candidate_index
                        && xedge.ty.0 == edge_ty_name_index
                        && !lc
                            .is_stale(query_context.look_at(edge_index.absolute()))
                            .unwrap()
                })
            }),
        // COPY_PASTE
        HasFreshIncomingEdgeOfType(edge_ty_name) => lc
            .string_table
            .get_index_of(edge_ty_name)
            .map_or(false, |edge_ty_name_index| {
                lc.raw_edges(query_context).any(|(edge_index, xedge)| {
                    xedge.to == candidate_index
                        && xedge.ty.0 == edge_ty_name_index
                        && !lc
                            .is_stale(query_context.look_at(edge_index.absolute()))
                            .unwrap()
                })
            }),
        Not(node_query_predicate) => !node_query_predicate.test(lc, candidate_index, query_context),
        }
    }
}

mod boilerplate;

// TODO bring tests up-to-date with the latest refactor
#[cfg(revise)]
#[cfg(test)]
mod test;
