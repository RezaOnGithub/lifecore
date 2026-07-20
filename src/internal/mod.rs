//! Implementation Details of Current LifeCoreList
use indexmap::IndexSet;
use itertools::Itertools;
use jiff::Unit;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Display,
    ops::Index,
    str::FromStr,
};
use strum::{EnumIs, EnumTryAs, VariantNames};
use whoami::{hostname, username};

use crate::{
    BasicValidationFailure, BasicValidationFailureScenario::*, Body, EdgeExt, EventContext,
    FamilyInsert, JSON, LifeCore, LifeCoreError, LifeCoreFamily, NodeExt, NodeType, Payload,
    PointedEventContext, Timestamp, TypedBody, Zoned, analysis,
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
struct NodeBody(BTreeMap<StringIndex, Payload>);

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
struct CircumstanceIndex(u64);

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
#[cfg_attr(test, derive(Debug))]
struct StringIndex(u64);

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
    fn last(&self) -> Option<PointedEventContext> {
        if self.event.is_empty() {
            None
        } else {
            Some(EventIndex((self.event.len() - 1) as u64).before_and_including())
        }
    }

    fn total_event_count(&self) -> usize {
        self.event.len()
    }

    fn connect(
        &mut self,
        from: PointedEventContext,
        ty: &str,
        to: PointedEventContext,
    ) -> Result<EdgeExt, LifeCoreError> {
        let mut candidate = self.clone();
        let circumstance = candidate
            .get_or_insert_current_session_circumstance_entry()?
            .0;
        let edge = candidate.connect_and_validate(from, ty, to, circumstance)?;
        *self = candidate;
        Ok(edge)
    }

    fn connect_mock(
        &mut self,
        from: PointedEventContext,
        ty: &str,
        to: PointedEventContext,
    ) -> Result<EdgeExt, LifeCoreError> {
        let mut candidate = self.clone();
        let circumstance = candidate.get_or_insert_mock_circumstance_entry().0;
        let edge = candidate.connect_and_validate(from, ty, to, circumstance)?;
        *self = candidate;
        Ok(edge)
    }

    fn add(
        &mut self,
        ty: &str,
        payload: Body,
        parent: Option<PointedEventContext>,
    ) -> Result<NodeExt, LifeCoreError> {
        let mut candidate = self.clone();
        let circumstance = candidate
            .get_or_insert_current_session_circumstance_entry()?
            .0;
        let node = candidate.add_and_validate(ty, payload, parent, circumstance)?;
        *self = candidate;
        Ok(node)
    }

    fn add_mock(
        &mut self,
        ty: &str,
        payload: Body,
        parent: Option<PointedEventContext>,
    ) -> Result<NodeExt, LifeCoreError> {
        let mut candidate = self.clone();
        let circumstance = candidate.get_or_insert_mock_circumstance_entry().0;
        let node = candidate.add_and_validate(ty, payload, parent, circumstance)?;
        *self = candidate;
        Ok(node)
    }

    fn is_mock(&self) -> bool {
        self.circumstance_table.contains(&Circumstance::Mock)
    }

    fn apply(&mut self) -> Result<(), LifeCoreError> {
        if !self.is_mock() {
            return Ok(());
        }

        let mut candidate = self.clone();
        candidate.basic_validation()?;
        let current_circumstance = candidate.current_session_circumstance()?;
        let mock_index = CircumstanceIndex((candidate.circumstance_table.len() - 1) as u64);

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

        let current_index = candidate
            .get_or_insert_circumstance_entry(current_circumstance)
            .0;
        if current_index != mock_index {
            for circumstance in candidate.circumstance.iter_mut() {
                if *circumstance == mock_index {
                    *circumstance = current_index;
                }
            }
        }

        candidate.basic_validation()?;
        *self = candidate;
        Ok(())
    }

    fn get(&self, e: PointedEventContext) -> Option<crate::EventExt> {
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
        I: IntoIterator<Item = PointedEventContext>,
    {
        i.into_iter().map_while(|x| self.get(x))
    }

    // TOTEST any remaining panic scenarios?
    fn is_stale(&self, e: PointedEventContext) -> Option<bool> {
        if e.thing >= self.event.len() {
            return None;
        }
        match &self[e.thing()] {
        Event::Node(_) => {
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
        Event::Edge(Edge { ty: _, from, to }) => {
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


    fn node(&self, ctx: PointedEventContext) -> Option<NodeExt> {
        let index: EventIndex = ctx.into();
        if !self[index].is_node() {
            return None;
        }
        let raw_node = self[index].as_raw_node();
        let ty = {
            if let Some((_, (_, ty))) = self
                .types(ctx.window())
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
            parent: raw_node
                .parent
                .map(|x| ctx.look_at(x.before_and_including())),
            e: ctx,
        })
    }

    fn edge(&self, ctx: PointedEventContext) -> Option<EdgeExt> {
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
    fn root_of(&self, e: PointedEventContext) -> Option<PointedEventContext> {
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

    fn fresh_successors(&self, node: PointedEventContext) -> Vec<PointedEventContext> {
        if self.get(node).is_none_or(|x| x.is_edge()) {
            return vec![];
        }
        let w = || node.exclusive_right_items();
        let mut successors = BTreeSet::<PointedEventContext>::from([node.absolute()]);
        let mut roots = BTreeSet::<PointedEventContext>::new();
        for (candidate_index, candidate) in w()
            .zip(self.raw_window(w()))
            .filter(|(_, event)| event.is_node())
            .map(|(i, e)| (i, e.as_raw_node()))
        {
            assert!(successors.intersection(&roots).nth(0).is_none());
            assert!(successors.iter().all(|x| *x == x.absolute()));
            assert!(roots.iter().all(|x| *x == x.absolute()));
            if let Some(candidate_parent) = candidate.parent {
                let candidate_parent: PointedEventContext = candidate_parent.absolute();
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

    fn relative_degree(
        &self,
        left: PointedEventContext,
        right: PointedEventContext,
    ) -> Option<usize> {
        let ancestor_chain = |mut n: PointedEventContext| {
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

    fn families(&self, ctx: EventContext) -> Vec<LifeCoreFamily> {
        let mut r = Vec::<LifeCoreFamily>::new();
        for (index, node) in self.raw_nodes(ctx) {
            if node.parent.is_none_or(|p| !ctx.range().contains(&p.i())) {
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
        e: PointedEventContext,
    ) -> Option<(crate::EventExt, crate::Circumstance)> {
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
    fn resolve(&self, t: &NodeBody) -> Body {
        let mut r = BTreeMap::<String, Payload>::new();
        for (k, v) in t.0.iter() {
            r.insert(self[k].to_string(), {
                match v {
                Payload::PreciseTime(timestamp) => Payload::PreciseTime(*timestamp),
                Payload::Text(string_index) => Payload::Text(self[string_index].to_string()),
                Payload::Number(i) => Payload::Number(*i),
                Payload::Serialized(string_index) => {
                    Payload::Serialized(JSON::from_str(&self[string_index]).unwrap())
                }
                }
            });
        }
        Body(r)
    }

    fn connect_and_validate(
        &mut self,
        from: PointedEventContext,
        ty: &str,
        to: PointedEventContext,
        circumstance: CircumstanceIndex,
    ) -> Result<EdgeExt, LifeCoreError> {
        let ty = self.get_or_insert_string(ty);
        self.event.push(Event::Edge(Edge {
            ty,
            from: from.thing(),
            to: to.thing(),
        }));
        self.circumstance.push(circumstance);
        self.basic_validation()?;
        Ok(self.edge(self.last().unwrap()).unwrap())
    }

    fn add_and_validate(
        &mut self,
        ty: &str,
        payload: Body,
        parent: Option<PointedEventContext>,
        circumstance: CircumstanceIndex,
    ) -> Result<NodeExt, LifeCoreError> {
        let ty = self.get_or_insert_string(ty);
        let mut body = NodeBody::default();
        for (name, payload) in payload.0.iter() {
            let name = self.get_or_insert_string(name);
            let payload = match payload {
            Payload::PreciseTime(timestamp) => Payload::PreciseTime(*timestamp),
            Payload::Text(value) => Payload::Text(self.get_or_insert_string(value.as_str())),
            Payload::Number(value) => Payload::Number(*value),
            Payload::Serialized(value) => {
                Payload::Serialized(self.get_or_insert_string(&value.to_string()))
            }
            };
            body.insert(name, payload);
        }
        self.event.push(Event::Node(Node {
            ty,
            parent: parent.map(|x| x.into()),
            payload: body,
        }));
        self.circumstance.push(circumstance);
        self.basic_validation()?;
        Ok(self.node(self.last().unwrap()).unwrap())
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

    fn raw_nodes(&self, ctx: EventContext) -> impl Iterator<Item = (PointedEventContext, Node)> {
        let w = || ctx.items();
        w().zip(self.raw_window(w()))
            .filter(|(_, event)| event.is_node())
            .map(|(index, event)| (index, event.as_raw_node()))
    }

    fn raw_edges(&self, ctx: EventContext) -> impl Iterator<Item = (PointedEventContext, Edge)> {
        let w = || ctx.items();
        w().zip(self.raw_window(w()))
            .filter(|(_, event)| event.is_edge())
            .map(|(index, event)| (index, event.as_raw_edge()))
    }

    fn raw_window<I>(&self, i: I) -> impl Iterator<Item = Event>
    where
        I: IntoIterator<Item = PointedEventContext>,
    {
        i.into_iter()
            .map_while(|x| self.event.get(x.thing).cloned())
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
