//! Implementation Details of Current LifeCoreList
use indexmap::IndexSet;
use itertools::Itertools;
use jiff::Unit;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fmt::Display,
    ops::Index,
    str::FromStr,
};
use strum::{Display, EnumIs, EnumTryAs, VariantNames};
use whoami::{hostname, username};

use crate::{
    BasicValidationFailure, BasicValidationFailureScenario::*, EdgeExt, EventContext, JSON,
    LifeCore, LifeCoreError, NodeBodyExt, NodeExt, NodeType, PayloadExt, Timestamp, TypedBody,
    Zoned, analysis,
};

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct LifeCoreList {
    // indexed by EventIndex
    event: Vec<Event>,
    circumstance: Vec<CircumstanceIndex>,

    // indexed by CircumstanceIndex
    circumstance_table: IndexSet<Circumstance>,

    // indexed by StringIndex
    string_table: IndexSet<String>,

    // Analyzers added so far. An analyzer has 2 pieces of state:
    // - initial config (just a small short string)
    // - nodes and edges that may get added as a part of the suggestions through AnalysisReport
    //   (may indicate "progression")
    configured_analyzer: analysis::SuiteConfig,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone)]
struct NodeBody(HashMap<StringIndex, Payload>);

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
struct CircumstanceIndex(u64);

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
#[cfg_attr(test, derive(Debug))]
struct StringIndex(u64);

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
#[cfg_attr(test, derive(Debug))]
struct EventIndex(u64);

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone, Hash)]
enum Circumstance {
    Mock,
    Session {
        when_1h: Zoned,
        hostname: StringIndex,
        username: StringIndex,
    },
}

#[derive(Serialize, Deserialize, EnumIs, EnumTryAs, Clone, PartialEq, Eq, Hash)]
enum Event {
    Node(Node),
    Edge(Edge),
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
struct Node {
    pub ty: StringIndex,
    pub parent: Option<EventIndex>,
    pub payload: NodeBody,
}
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(test, derive(Debug))]
struct Edge {
    pub ty: StringIndex,
    pub from: EventIndex,
    pub to: EventIndex,
}

// This is for the internal representation of Node Payload
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, VariantNames)]
enum Payload {
    PreciseTime(Timestamp),
    Text(StringIndex),
    Number(i64),

    // NOTE
    Serialized(StringIndex),
}

impl LifeCore for LifeCoreList {
    fn last(&self) -> Option<EventContext> {
        if self.event.is_empty() {
            None
        } else {
            Some(EventIndex((self.event.len() - 1) as u64).before_and_including())
        }
    }

    fn connect(
        &mut self,
        from: EventContext,
        ty: &str,
        to: EventContext,
    ) -> Result<EdgeExt, LifeCoreError> {
        // Perform action on a copy. If it is valid, perform it on self
        {
            let mut t = self.clone();

            let circumstance_index = t.get_or_insert_mock_circumstance_entry().0;
            let ty_str_index = t.get_or_insert_string(ty);
            t.event.push(Event::Edge(Edge {
                ty: ty_str_index,
                from: from.thing(),
                to: to.thing(),
            }));
            t.circumstance.push(circumstance_index);
            let _ = t.basic_validation()?;
            let _ = t.edge(t.last().unwrap());
        }

        // COPY_PASTE
        // Circumstance MUST come first, because if it fails somehow, then we inserted
        // intermediate data for nothing
        let current_circumstance_index = self.get_or_insert_current_session_circumstance_entry()?.0;
        let ty_str_index = self.get_or_insert_string(ty);
        self.event.push(Event::Edge(Edge {
            ty: ty_str_index,
            from: from.into(),
            to: to.into(),
        }));
        self.circumstance.push(current_circumstance_index);
        assert!(self.basic_validation().is_ok());
        Ok(self.edge(self.last().unwrap()).unwrap())
    }

    fn add(
        &mut self,
        ty: &str,
        payload: NodeBodyExt,
        parent: Option<EventContext>,
    ) -> Result<NodeExt, LifeCoreError> {
        let insert_all_strings = |t: &mut Self, payload: &NodeBodyExt| -> NodeBody {
            let mut r = NodeBody::default();
            for (k, v) in payload.0.iter() {
                let smalltable_key = t.get_or_insert_string(k);
                let smalltable_value = {
                    match v {
                    PayloadExt::PreciseTime(timestamp) => Payload::PreciseTime(*timestamp),
                    PayloadExt::Text(s) => Payload::Text(t.get_or_insert_string(s.as_str())),
                    PayloadExt::Number(i) => Payload::Number(*i),
                    PayloadExt::Serialized(s) => {
                        Payload::Serialized(t.get_or_insert_string(&s.to_string()))
                    }
                    }
                };
                r.insert(smalltable_key, smalltable_value);
            }
            r
        };

        {
            let mut t = self.clone();
            // NOTE order of these two operations is SUS
            let circumstance_index = t.get_or_insert_mock_circumstance_entry().0;
            let ty_str_index = t.get_or_insert_string(ty);
            let p = insert_all_strings(&mut t, &payload);
            t.event.push(Event::Node(Node {
                ty: ty_str_index,
                parent: parent.clone().map(|x| x.into()),
                payload: p,
            }));
            t.circumstance.push(circumstance_index);
            let _ = t.basic_validation()?;
            let _ = t.node(t.last().unwrap());
        }

        // COPY_PASTE
        let circumstance_index = self.get_or_insert_current_session_circumstance_entry()?.0;
        let ty_str_index = self.get_or_insert_string(ty);
        let p = insert_all_strings(self, &payload);
        self.event.push(Event::Node(Node {
            ty: ty_str_index,
            parent: parent.clone().map(|x| x.into()),
            payload: p,
        }));
        self.circumstance.push(circumstance_index);

        assert!(self.basic_validation().is_ok());
        Ok(self.node(self.last().unwrap()).unwrap())
    }

    fn get(&self, e: EventContext) -> Option<crate::EventExt> {
        let index: EventIndex = e.into();
        if index.i() >= self.event.len() {
            return None;
        }
        match self[index] {
        Event::Node(_) => Some(crate::EventExt::Node(self.node(e)?)),
        Event::Edge(_) => Some(crate::EventExt::Edge(self.edge(e)?)),
        }
    }

    fn window<I>(&self, i: I) -> impl Iterator<Item = crate::EventExt>
    where
        I: IntoIterator<Item = EventContext>,
    {
        i.into_iter().map_while(|x| self.get(x))
    }

    fn is_stale(&self, e: EventContext) -> bool {
        let ctx = || e.items();
        match self[e.thing()] {
        Event::Node(_) => {
            for potential_child_node in self.raw_window(ctx()).filter_map(|event| {
                if event.is_node() {
                    Some(event.as_raw_node())
                } else {
                    None
                }
            }) {
                if let Some(idx) = potential_child_node.parent
                    && idx == e.thing()
                {
                    return true;
                }
            }
        }
        Event::Edge(Edge { ty: _, from, to }) => {
            // SUS
            if self.is_stale(e.look_at(from.before_and_including()))
                || self.is_stale(e.look_at(to.before_and_including()))
            {
                return true;
            }
        }
        }
        false
    }

    fn basic_validation(&self) -> Result<(), BasicValidationFailure> {
        let ctx = || EventContext::universe();

        // Non-Rules. Have little to do with the meaning of the graph, but more related
        // to data integrity
        {
            // Payload and PayloadExt cover the same types
            {
                assert!(Payload::VARIANTS.to_vec() == PayloadExt::VARIANTS.to_vec())
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

            // Every Event happens under a Circumstance
            {
                if self.event.len() != self.circumstance.len() {
                    return Err(BasicValidationFailure {
                        offending_event: ctx(),
                        reason: InternalError {
                            message: "Circumstance list does not match up with Event list"
                                .to_string(),
                        },
                    });
                }
            }

            // Only a SINGLE Circumstance::Mock is allowed. This is only for running
            // validation code before adding a new event TODO modify this so
            // that only TAIL Circumstance::Mock are allowed.
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
                        .filter(|x| x.1.0 as usize == mock_index)
                        .collect_vec();
                    assert!(!mock_event_index.is_empty());
                    if mock_event_index.len() > 1 || mock_event_index[0].0 != self.event.len() - 1 {
                        // TODO better context
                        return Err(BasicValidationFailure {
                            offending_event: ctx(),
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
                    offending_event: ctx(),
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
                        if pair[0].0.thing() < pair[1].0.thing() {
                            (pair[0], pair[1])
                        } else {
                            (pair[1], pair[0])
                        }
                    };

                    if self.is_stale(newer_index.shrink_to_exclusive_left().look_at(*older_index)) {
                        // TODO move this Warning into its own analyzer
                        // warn!(
                        //     *newer_index,
                        //     "Duplicate nodes, but older one was invalidated
                        // before the new one was added (discouraged).
                        // EventIndex {} and {}",
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
                                && idx == older_index.thing()
                            {
                                false
                            } else {
                                true
                            }
                        });
                        return Err(BasicValidationFailure {
                            offending_event: *newer_index,
                            reason: DuplicateNode,
                        });
                    }
                }
            }
        }

        // Rule: events are sorted, nodes and edges cannot refer to themselves or the
        // future Rule: Edges can only be between nodes
        // Rule: Node parent can only be another node

        for e in self.event.iter().enumerate() {
            let idx_in_range = |idx: &EventIndex| (idx.0 as usize) < e.0;
            match e.1 {
            Event::Node(Node {
                ty: _,
                parent,
                payload: _,
            }) => {
                if let Some(idx) = parent {
                    if !idx_in_range(idx) {
                        return Err(BasicValidationFailure {
                            offending_event: EventContext::universe()
                                .look_at(idx.before_and_including()),
                            reason: OutOfRangeParent,
                        });
                    }
                    if self[idx].is_edge() {
                        return Err(BasicValidationFailure {
                            offending_event: EventContext::universe()
                                .look_at(idx.before_and_including()),
                            reason: EdgeParent,
                        });
                    }
                }
            }
            Event::Edge(Edge { ty: _, from, to }) => {
                let idx = EventIndex::from(e.0);
                if !idx_in_range(from) || !idx_in_range(to) {
                    return Err(BasicValidationFailure {
                        offending_event: EventContext::universe()
                            .look_at(idx.before_and_including()),
                        reason: OutOfRangeEdgeEnd,
                    });
                }
                if self[from].is_edge() || self[to].is_edge() {
                    return Err(BasicValidationFailure {
                        offending_event: EventContext::universe()
                            .look_at(idx.before_and_including()),
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
            assert!(edge_context.thing().0 > edge.from.0 && edge_context.thing().0 > edge.to.0);
            if self.is_stale(ctx.look_at(edge.from.before_and_including()))
                || self.is_stale(ctx.look_at(edge.to.before_and_including()))
            {
                return Err(BasicValidationFailure {
                    offending_event: EventContext::universe().look_at(edge_context.into()),
                    reason: StaleRuleViolation,
                });
            }
        }

        Ok(())
    }

    fn node(&self, ctx: EventContext) -> Option<NodeExt> {
        let index: EventIndex = ctx.into();
        if !self[index].is_node() {
            return None;
        }
        let raw_node = self[index].as_raw_node();
        let ty = {
            if let Some((_, (_, ty))) = self
                .types(ctx)
                .filter(|(name, _)| name.as_str() == &self[raw_node.ty])
                .nth(0)
            {
                Some(ty)
            } else {
                None
            }
        };
        let body = self.resolve(&raw_node.payload);
        Some(NodeExt {
            ty,
            ty_name: self[raw_node.ty].to_string(),
            field: body,
            parent: raw_node.parent.map(|x| x.before_and_including()),
            e: ctx,
        })
    }

    fn edge(&self, ctx: EventContext) -> Option<EdgeExt> {
        let index: EventIndex = ctx.into();
        if !self[index].is_edge() {
            return None;
        }
        let raw_edge = self[index].as_raw_edge();
        Some(EdgeExt {
            ty_name: self[raw_edge.ty].to_string(),
            from: ctx.look_at(raw_edge.from.before_and_including()),
            to: ctx.look_at(raw_edge.to.before_and_including()),
            e: ctx,
        })
    }

    // NOTE `ctx` can be out of bounds! Be careful!
    // Wrong approaches:
    // - `ctx.items().flat_map(...)`
    fn nodes(&self, ctx: EventContext) -> impl Iterator<Item = NodeExt> {
        let w = || ctx.items();
        w().zip(self.window(w()))
            .filter(|(_, event)| event.is_node())
            .map(move |(index, _)| self.node(ctx.look_at(index)).unwrap())
    }

    // COPY_PASTE
    fn edges(&self, ctx: EventContext) -> impl Iterator<Item = EdgeExt> {
        let w = || ctx.items();
        w().zip(self.window(w()))
            .filter(|(_, event)| event.is_edge())
            .map(move |(index, _)| self.edge(ctx.look_at(index)).unwrap())
    }

    /// Get the root of `e`. `None` if `e` is a root node or an edge
    fn root_of(&self, e: EventContext) -> Option<EventContext> {
        let index: EventIndex = e.into();
        if self[index].is_edge() {
            return None;
        }
        let mut r = index;
        while let Some(parent_event) = self.raw_node(&r).parent {
            r = parent_event;
        }
        Some(e.look_raw(r.i()))
    }

    // TODO revise
    // The current implementation is wrong because we need to look at all
    // successors, not just immediate successors. Need to make this clear in the
    // documentation too. Can contain itself, if it does not have any children
    fn fresh_successors(&self, node: EventContext) -> Vec<EventContext> {
        if self.get(node).is_none_or(|x| x.is_edge()) {
            return vec![];
        }
        let w = || node.exclusive_right_items();
        let mut successor = HashSet::<EventContext>::from([node.absolute()]);
        for (candidate_index, candidate) in w()
            .zip(self.raw_window(w()))
            .filter(|(_, event)| event.is_node())
            .map(|(i, e)| (i, e.as_raw_node()))
        {
            if candidate
                .parent
                .is_some_and(|parent| successor.contains(&parent.absolute()))
            {
                successor.remove(&candidate.parent.unwrap().absolute());
                successor.insert(candidate_index.absolute());
            }
        }
        successor.into_iter().collect()
    }

    fn relative_degree(&self, left: EventContext, right: EventContext) -> Option<usize> {
        let ancestor_chain = |mut n: EventContext| {
            let mut chain = vec![];
            while let Some(parent) = self.node(n).unwrap().parent {
                let parent = n.look_at(parent);
                chain.push(parent.absolute());
                n = parent;
            }
            chain
        };

        if !self.get(left).is_some_and(|x| x.is_node())
            || !self.get(right).is_some_and(|x| x.is_node())
        {
            return None;
        }

        let left_ancestors = ancestor_chain(left);
        let right_ancestors = ancestor_chain(right);
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

    fn types(&self, e: EventContext) -> impl Iterator<Item = (String, (EventContext, NodeType))> {
        e.items()
            .zip(self.raw_window(e.items()))
            .filter_map(|(ctx, event)| {
                if event.is_node()
                    && &self[event.as_raw_node().ty] == NodeType::name()
                    && !self.is_stale(ctx)
                {
                    if let Ok(ty) = NodeType::try_from(&self.resolve(&event.as_raw_node().payload))
                    {
                        Some((ty.name.clone(), (ctx, ty)))
                    } else {
                        // We have much bigger problems
                        None
                    }
                } else {
                    None
                }
            })
    }

    fn non_solo_families(&self, ctx: EventContext) -> Vec<Vec<EventContext>> {
        let mut family = Vec::<HashSet<EventIndex>>::new();
        'outer: for (index, node) in self.raw_nodes(ctx) {
            if node.parent.is_none_or(|p| !ctx.range().contains(&p.i())) {
                // We consider this a root node
                let mut r = HashSet::new();
                r.insert(index.into());
                family.push(r);
            } else {
                // This must be a child node
                for set_index in 0..family.len() {
                    if family[set_index].contains(&node.parent.unwrap()) {
                        family.get_mut(set_index).unwrap().insert(index.into());
                        continue 'outer;
                    }
                }
                panic!(
                    "parent {} for this node {} should had been found by now",
                    node.parent.unwrap(),
                    index
                );
            }
        }
        family
            .into_iter()
            .map(|x| {
                x.into_iter()
                    .map(|y| ctx.look_at(y.before_and_including()))
                    .collect_vec()
            })
            .collect_vec()
    }

    fn add_analyzer(&mut self, name: &str, config: &str) {
        self.configured_analyzer.0.push(analysis::Config {
            name: name.to_string(),
            config: config.to_string(),
        });
    }

    fn current_analysis_suite(&self) -> analysis::SuiteConfig {
        self.configured_analyzer.clone()
    }

    fn get_with_circumstance(
        &self,
        e: EventContext,
    ) -> Option<(crate::EventExt, crate::CircumstanceExt)> {
        let i: EventIndex = e.into();
        let e = self.get(e)?;
        Some((
            e,
            self.resolve_circumstance(
                self.circumstance_table
                    .get_index(self.circumstance[i.0 as usize].0 as usize)?,
            ),
        ))
    }
}

impl Index<&StringIndex> for LifeCoreList {
    type Output = str;

    fn index(&self, index: &StringIndex) -> &Self::Output {
        self.string_table
            .get_index(index.0 as usize)
            .expect("StringIndex out of range")
    }
}

impl Index<StringIndex> for LifeCoreList {
    type Output = str;

    fn index(&self, index: StringIndex) -> &Self::Output {
        &self[&index]
    }
}

impl Index<&EventIndex> for LifeCoreList {
    type Output = Event;

    fn index(&self, index: &EventIndex) -> &Self::Output {
        self.event
            .get(index.0 as usize)
            .expect("EventIndex out of range")
    }
}

impl Index<EventIndex> for LifeCoreList {
    type Output = Event;

    fn index(&self, index: EventIndex) -> &Self::Output {
        &self[&index]
    }
}

impl Index<&CircumstanceIndex> for LifeCoreList {
    type Output = Circumstance;

    fn index(&self, index: &CircumstanceIndex) -> &Self::Output {
        self.circumstance_table
            .get_index(index.0 as usize)
            .expect("StringIndex out of range")
    }
}

impl Index<CircumstanceIndex> for LifeCoreList {
    type Output = Circumstance;

    fn index(&self, index: CircumstanceIndex) -> &Self::Output {
        &self[&index]
    }
}

impl LifeCoreList {
    fn resolve(&self, t: &NodeBody) -> NodeBodyExt {
        let mut r = HashMap::<String, PayloadExt>::new();
        for (k, v) in t.0.iter() {
            r.insert(self[k].to_string(), {
                match v {
                Payload::PreciseTime(timestamp) => PayloadExt::PreciseTime(*timestamp),
                Payload::Text(string_index) => PayloadExt::Text(self[string_index].to_string()),
                Payload::Number(i) => PayloadExt::Number(*i),
                Payload::Serialized(string_index) => {
                    PayloadExt::Serialized(JSON::from_str(&self[string_index]).unwrap())
                }
                }
            });
        }
        NodeBodyExt(r)
    }

    fn get_or_insert_current_session_circumstance(
        &mut self,
    ) -> Result<crate::CircumstanceExt, LifeCoreError> {
        let (_, circumstance) = self.get_or_insert_current_session_circumstance_entry()?;
        Ok(self.resolve_circumstance(&circumstance))
    }

    fn get_or_insert_mock_circumstance(&mut self) -> Result<crate::CircumstanceExt, LifeCoreError> {
        let (_, circumstance) = self.get_or_insert_mock_circumstance_entry();
        Ok(self.resolve_circumstance(&circumstance))
    }

    fn get_or_insert_current_session_circumstance_entry(
        &mut self,
    ) -> Result<(CircumstanceIndex, Circumstance), LifeCoreError> {
        let now = Zoned::now().round(Unit::Hour)?;
        let hname = hostname()?;
        let uname = username()?;
        let hname_index = self.get_or_insert_string(hname.as_str());
        let uname_index = self.get_or_insert_string(uname.as_str());
        let circumstance = Circumstance::Session {
            when_1h: now,
            hostname: hname_index,
            username: uname_index,
        };
        if let Some(idx) = self.circumstance_table.get_index_of(&circumstance) {
            Ok((CircumstanceIndex(idx as u64), circumstance))
        } else {
            assert!(
                self.circumstance_table.insert(circumstance.clone()),
                "Circumstance should not be present. We just checked!"
            );
            Ok((
                CircumstanceIndex((self.circumstance_table.len() - 1) as u64),
                circumstance,
            ))
        }
    }

    fn get_or_insert_mock_circumstance_entry(&mut self) -> (CircumstanceIndex, Circumstance) {
        if let Some(idx) = self.circumstance_table.get_index_of(&Circumstance::Mock) {
            (CircumstanceIndex(idx as u64), Circumstance::Mock)
        } else {
            assert!(
                self.circumstance_table.insert(Circumstance::Mock),
                "Circumstance should not be present. We just checked!"
            );
            (
                CircumstanceIndex((self.circumstance_table.len() - 1) as u64),
                Circumstance::Mock,
            )
        }
    }

    fn get_or_insert_string(&mut self, s: &str) -> StringIndex {
        let x = self.string_table.get_index_of(s);
        if let Some(existing_index) = x {
            StringIndex(existing_index as u64)
        } else {
            assert!(self.string_table.insert(s.to_string()));
            StringIndex((self.string_table.len() - 1) as u64)
        }
    }

    fn resolve_circumstance(&self, circumstance: &Circumstance) -> crate::CircumstanceExt {
        match circumstance {
        Circumstance::Mock => crate::CircumstanceExt::Mock,
        Circumstance::Session {
            when_1h,
            hostname,
            username,
        } => crate::CircumstanceExt::Session {
            when_1h: when_1h.clone(),
            hostname: self[*hostname].to_string(),
            username: self[*username].to_string(),
        },
        }
    }

    fn event(&self, i: &EventIndex) -> (Circumstance, Event) {
        (
            self.circumstance_table
                .get_index(self.circumstance[i.0 as usize].0 as usize)
                .expect("CirucmstanceIndex out of range")
                .clone(),
            self[i].clone(),
        )
    }

    fn raw_node(&self, i: &EventIndex) -> Node {
        self[*i].as_raw_node()
    }

    fn raw_edge(&self, i: &EventIndex) -> Edge {
        self[*i].as_raw_edge()
    }

    fn raw_nodes(&self, ctx: EventContext) -> impl Iterator<Item = (EventContext, Node)> {
        let w = || ctx.items();
        w().zip(self.raw_window(w()))
            .filter(|(_, event)| event.is_node())
            .map(|(index, event)| (index, event.as_raw_node()))
    }

    fn raw_edges(&self, ctx: EventContext) -> impl Iterator<Item = (EventContext, Edge)> {
        let w = || ctx.items();
        w().zip(self.raw_window(w()))
            .filter(|(_, event)| event.is_edge())
            .map(|(index, event)| (index, event.as_raw_edge()))
    }

    fn raw_window<I>(&self, i: I) -> impl Iterator<Item = Event>
    where
        I: IntoIterator<Item = EventContext>,
    {
        i.into_iter()
            .map_while(|x| self.event.get(x.thing.unwrap()).cloned())
    }
}

impl Default for LifeCoreList {
    fn default() -> Self {
        Self {
            event: vec![],
            circumstance: vec![],
            circumstance_table: IndexSet::new(),
            string_table: IndexSet::new(),
            configured_analyzer: analysis::SuiteConfig(vec![]),
        }
    }
}

impl EventContext {
    fn thing(self) -> EventIndex {
        self.into()
    }
}

impl EventIndex {
    fn before(&self) -> EventContext {
        EventContext {
            end: self.i(),
            thing: None,
        }
    }

    fn before_and_including(&self) -> EventContext {
        EventContext {
            end: self.i().saturating_add(1),
            thing: Some(self.i()),
        }
    }

    fn absolute(&self) -> EventContext {
        EventContext::universe().look_raw(self.i())
    }

    fn i(self) -> usize {
        self.0 as usize
    }
}

impl From<EventContext> for EventIndex {
    fn from(value: EventContext) -> Self {
        assert!(value.validate() && value.thing.is_some());
        Self(u64::try_from(value.thing.unwrap()).unwrap())
    }
}

impl From<&EventContext> for EventIndex {
    fn from(value: &EventContext) -> Self {
        Self::from(*value)
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

impl From<&EventIndex> for usize {
    fn from(val: &EventIndex) -> Self {
        val.0 as usize
    }
}

impl From<EventIndex> for usize {
    fn from(val: EventIndex) -> Self {
        (&val).into()
    }
}

impl Display for EventIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(format!("EventIndex({})", self.0 as usize).as_str())
    }
}

impl Event {
    pub(crate) fn as_raw_node(&self) -> Node {
        self.clone().try_as_node().unwrap()
    }

    pub(crate) fn as_raw_edge(&self) -> Edge {
        self.clone().try_as_edge().unwrap()
    }
}

impl Default for NodeBody {
    fn default() -> Self {
        Self(Default::default())
    }
}

impl NodeBody {
    fn insert(&mut self, key: StringIndex, val: Payload) {
        assert!(!self.0.contains_key(&key));
        assert!(self.0.insert(key, val).is_none());
    }
}

impl std::hash::Hash for NodeBody {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let mut x = self.0.iter().collect_vec();
        x.sort_by(|a, b| a.0.cmp(b.0));
        x.hash(state);
    }
}

#[cfg(test)]
mod test;
