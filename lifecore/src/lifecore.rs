use indexmap::IndexSet;
use itertools::Itertools;
use jiff::{Unit, Zoned};
use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    ops::Index,
};
use strum::VariantNames;
use whoami::{hostname, username};

use crate::{
    Circumstance, CircumstanceIndex, Edge, Event, EventIndex, LifeCore, Node, NodeBody, Payload,
    PayloadExt, SerializedExt, SmallTable, StringIndex, analyzer::AnalysisSuite, to_map,
};

impl Index<&StringIndex> for LifeCore {
    type Output = str;

    fn index(&self, index: &StringIndex) -> &Self::Output {
        self.string_table
            .get_index(index.0 as usize)
            .expect("StringIndex out of range")
    }
}

impl Index<StringIndex> for LifeCore {
    type Output = str;

    fn index(&self, index: StringIndex) -> &Self::Output {
        &self[&index]
    }
}

impl Index<&EventIndex> for LifeCore {
    type Output = Event;

    fn index(&self, index: &EventIndex) -> &Self::Output {
        self.event
            .get(index.0 as usize)
            .expect("EventIndex out of range")
    }
}

impl Index<EventIndex> for LifeCore {
    type Output = Event;

    fn index(&self, index: EventIndex) -> &Self::Output {
        &self[&index]
    }
}

impl Index<&CircumstanceIndex> for LifeCore {
    type Output = Circumstance;

    fn index(&self, index: &CircumstanceIndex) -> &Self::Output {
        self.circumstance_table
            .get_index(index.0 as usize)
            .expect("StringIndex out of range")
    }
}

impl Index<CircumstanceIndex> for LifeCore {
    type Output = Circumstance;

    fn index(&self, index: CircumstanceIndex) -> &Self::Output {
        &self[&index]
    }
}

impl LifeCore {
    // TODO bootstrap types
    pub fn new() -> Self {
        Self {
            event: vec![],
            circumstance: vec![],
            circumstance_table: IndexSet::new(),
            string_table: IndexSet::new(),
            analyzer: AnalysisSuite(vec![]),
        }
    }

    // get the INDEX for the last event
    pub fn last(&self) -> EventIndex {
        assert!(!self.event.is_empty());
        EventIndex((self.total_event_count() - 1) as u64)
    }

    pub fn total_event_count(&self) -> usize {
        self.event.len()
    }

    pub fn connect(
        &mut self,
        from: EventIndex,
        ty: &str,
        to: EventIndex,
    ) -> anyhow::Result<EventIndex> {
        // Perform action on a copy. If it is valid, perform it on self
        // NOTE: we do not insert a new Circumstance yet as the checks could take a long time- and we rely on the fact that there is minimal validation around Circumstance
        {
            let mut t = self.clone();

            let circumstance_index = t.get_or_insert_mock_circumstance().0;
            let ty_str_index = t.get_or_insert_string(ty);
            t.event.push(Event::Edge(Edge {
                ty: ty_str_index,
                from,
                to,
            }));
            t.circumstance.push(circumstance_index);
            let _simulate = t.basic_validation()?;
        }

        // NOTE COPY_PASTE
        // Circumstance MUST come first, because if it fails somehow, then we inserted intermediate data for nothing
        let current_circumstance_index = self.get_or_insert_current_session_circumstance()?.0;
        let ty_str_index = self.get_or_insert_string(ty);
        self.event.push(Event::Edge(Edge {
            ty: ty_str_index,
            from,
            to,
        }));
        self.circumstance.push(current_circumstance_index);
        assert!(self.basic_validation().is_ok());
        Ok(self.last())
    }

    pub fn add(
        &mut self,
        ty: &str,
        payload: HashMap<&str, PayloadExt>,
        parent: Option<EventIndex>,
    ) -> anyhow::Result<EventIndex> {
        let insert_all_strings = |t: &mut Self,
                                  payload: &HashMap<&str, PayloadExt>|
         -> SmallTable<StringIndex, Payload> {
            let mut r: SmallTable<StringIndex, Payload> = SmallTable::new();
            for (k, v) in payload {
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
            let circumstance_index = t.get_or_insert_mock_circumstance().0;
            let ty_str_index = t.get_or_insert_string(ty);
            let p = insert_all_strings(&mut t, &payload);
            t.event.push(Event::Node(Node {
                ty: ty_str_index,
                parent,
                payload: p,
            }));
            t.circumstance.push(circumstance_index);
            let _simulate = t.basic_validation()?;
        }

        // NOTE COPY_PASTE
        let circumstance_index = self.get_or_insert_current_session_circumstance()?.0;
        let ty_str_index = self.get_or_insert_string(ty);
        let p = insert_all_strings(self, &payload);
        self.event.push(Event::Node(Node {
            ty: ty_str_index,
            parent,
            payload: p,
        }));
        self.circumstance.push(circumstance_index);

        assert!(self.basic_validation().is_ok());
        Ok(self.last())
    }

    pub fn resolve(&self, t: &SmallTable<StringIndex, Payload>) -> NodeBody {
        let mut r = HashMap::<String, PayloadExt>::new();
        for (k, v) in t.0.iter().filter(|x| x.is_some()).map(|x| x.unwrap()) {
            r.insert(self[k].to_string(), {
                match v {
                    Payload::PreciseTime(timestamp) => PayloadExt::PreciseTime(timestamp),
                    Payload::Text(string_index) => PayloadExt::Text(self[string_index].to_string()),
                    Payload::Number(i) => PayloadExt::Number(i),
                    Payload::Serialized(string_index) => PayloadExt::Serialized(
                        SerializedExt::try_from(serde_json::Value::from(&self[string_index]))
                            .unwrap(),
                    ),
                }
            });
        }
        NodeBody(r)
    }

    pub fn get_or_insert_current_session_circumstance(
        &mut self,
    ) -> anyhow::Result<(CircumstanceIndex, Circumstance)> {
        let now = Zoned::now().round(Unit::Hour)?;
        let hname = hostname()?;
        let uname = username()?;
        // CANNOT actually use `?` because it would attempt to return a reference to strings allocated locally
        let hname_index = self.get_or_insert_string(hname.as_str());
        let uname_index = self.get_or_insert_string(uname.as_str());
        let circumstance = Circumstance::Session {
            when_1h: now,
            hostname: hname_index,
            username: uname_index,
        };
        if let Some(idx) = self.circumstance_table.get_index_of(&circumstance) {
            anyhow::Result::Ok((CircumstanceIndex(idx as u64), circumstance))
        } else {
            self.circumstance_table.insert(circumstance.clone());
            anyhow::Result::Ok((
                CircumstanceIndex((self.circumstance_table.len() - 1) as u64),
                circumstance,
            ))
        }
    }

    pub fn get_or_insert_mock_circumstance(&mut self) -> (CircumstanceIndex, Circumstance) {
        if let Some(idx) = self.circumstance_table.get_index_of(&Circumstance::Mock) {
            (CircumstanceIndex(idx as u64), Circumstance::Mock)
        } else {
            self.circumstance_table.insert(Circumstance::Mock);
            (
                CircumstanceIndex((self.circumstance_table.len() - 1) as u64),
                Circumstance::Mock,
            )
        }
    }

    pub fn get_or_insert_string(&mut self, s: &str) -> StringIndex {
        let x = self.string_table.get_index_of(s);
        if x.is_some() {
            StringIndex(x.unwrap() as u64)
        } else {
            self.string_table.insert(s.to_string());
            StringIndex((self.string_table.len() - 1) as u64)
        }
    }

    // Accessor Zoo

    pub fn node(&self, e: EventIndex) -> Node {
        self[e].clone().try_as_node().unwrap()
    }

    pub fn edge(&self, e: EventIndex) -> Edge {
        self[e].clone().try_as_edge().unwrap()
    }

    pub fn root_of(&self, e: EventIndex) -> Option<EventIndex> {
        if self[e].is_edge() {
            return None;
        }
        let mut r = e;
        while let Some(parent_event) = self.node(r).parent {
            r = parent_event;
        }
        Some(r)
    }

    pub fn edges(&self) -> impl Iterator<Item = (EventIndex, Edge)> {
        self.event
            .iter()
            .enumerate()
            .filter(|x| x.1.is_edge())
            .map(|x| (x.0.into(), self.edge(x.0.into())))
    }

    pub fn nodes(&self) -> impl Iterator<Item = (EventIndex, Node)> {
        self.event
            .iter()
            .enumerate()
            .filter(|x| x.1.is_node())
            .map(|x| (x.0.into(), self.node(x.0.into())))
    }

    pub fn nodes_in_range(
        &self,
        range: std::ops::Range<usize>,
    ) -> impl Iterator<Item = (EventIndex, Node)> {
        assert!(range.end >= range.start);
        self.event
            .iter()
            .enumerate()
            .skip(range.start)
            .take(range.end - range.start)
            .filter(|x| x.1.is_node())
            .map(|x| (x.0.into(), x.1.clone().try_as_node().unwrap()))
    }

    pub fn edges_in_range(
        &self,
        range: std::ops::Range<usize>,
    ) -> impl Iterator<Item = (EventIndex, Edge)> {
        assert!(range.end >= range.start);
        self.event
            .iter()
            .enumerate()
            .skip(range.start)
            .take(range.end - range.start)
            .filter(|x| x.1.is_edge())
            .map(|x| (EventIndex(x.0 as u64), x.1.clone().try_as_edge().unwrap()))
    }

    pub fn fresh_nodes_in_range(
        &self,
        range: std::ops::Range<usize>,
    ) -> impl Iterator<Item = (EventIndex, Node)> {
        self.nodes_in_range(range.clone()).filter(move |(e, _)| {
            !(range.clone().count() >= 1
                && self
                    .is_stale_by_and_including(*e, EventIndex::from(range.clone().last().unwrap())))
        })
    }

    pub fn fresh_edges_in_range(
        &self,
        range: std::ops::Range<usize>,
    ) -> impl Iterator<Item = (EventIndex, Edge)> {
        self.edges_in_range(range.clone()).filter(move |(e, _)| {
            !(range.clone().count() >= 1
                && self
                    .is_stale_by_and_including(*e, EventIndex::from(range.clone().last().unwrap())))
        })
    }

    pub fn nodes_in_range_of_type(
        &self,
        ty: &str,
        range: std::ops::Range<usize>,
    ) -> impl Iterator<Item = (EventIndex, Node)> {
        range
            .clone()
            .into_iter()
            .filter(move |e| self.event[*e].is_node() && &self[self.node(e.into()).ty] == ty)
            .map(|x| (x.into(), self.node(x.into())))
    }

    pub fn edges_in_range_of_type(
        &self,
        ty: &str,
        range: std::ops::Range<usize>,
    ) -> impl Iterator<Item = (EventIndex, Edge)> {
        range
            .clone()
            .into_iter()
            .filter(move |e| self.event[*e].is_edge() && &self[self.edge(e.into()).ty] == ty)
            .map(|x| (x.into(), self.edge(x.into())))
    }

    pub fn fresh_nodes_in_range_of_type(
        &self,
        ty: &str,
        range: std::ops::Range<usize>,
    ) -> impl Iterator<Item = (EventIndex, Node)> {
        range
            .clone()
            .into_iter()
            .filter(move |e| {
                self.event[*e].is_node()
                    && &self[self.edge(e.into()).ty] == ty
                    && !self.is_stale_by_and_including(e.into(), (range.end - 1).into())
            })
            .map(|x| (x.into(), self.node(x.into())))
    }

    pub fn fresh_edges_in_range_of_type(
        &self,
        ty: &str,
        range: std::ops::Range<usize>,
    ) -> impl Iterator<Item = (EventIndex, Edge)> {
        // NOTE if range.count() >= 1 then (range.end - 1) is always valid
        range
            .clone()
            .into_iter()
            .filter(move |e| {
                self.event[*e].is_edge()
                    && &self[self.edge(e.into()).ty] == ty
                    && !self.is_stale_by_and_including(e.into(), (range.end - 1).into())
            })
            .map(|x| (x.into(), self.edge(x.into())))
    }

    // Check whether a `e` has become stale within `0..=upper` (upper inclusive)
    // Stale Node means a newer node has it as its parent
    // Stale Edge means at least one end is a Stale Node
    // NOTE in Stale Node case, upper could refer to the child that makes `e` stale (parent of upper == e)
    // TODO check if the above fact is considered where this is called
    pub fn is_stale_by_and_including(&self, e: EventIndex, upper: EventIndex) -> bool {
        match self[e] {
            Event::Node(_) => {
                let window = &self.event[upper.before_and_including()];
                for interim_node in window
                    .iter()
                    .filter(|x| x.is_node())
                    .map(|x| x.clone().try_as_node().unwrap())
                {
                    if let Some(idx) = interim_node.parent
                        && idx == e
                    {
                        return true;
                    }
                }
            }
            Event::Edge(Edge { ty: _, from, to }) => {
                if self.is_stale_by_and_including(from, upper)
                    || self.is_stale_by_and_including(to, upper)
                {
                    return true;
                }
            }
        }
        false
    }

    pub fn is_stale(&self, e: EventIndex) -> bool {
        self.is_stale_by_and_including(e, self.last())
    }

    // Return nodes that have a predecessor, with the predecessors
    // TODO no easy way of keeping borrowchecker happy without .collect_vec
    // TODO PERF
    pub fn non_solo_families(&self) -> Vec<Vec<EventIndex>> {
        self.nodes()
            .into_group_map_by(|x| self.root_of(x.0))
            .into_values()
            .filter(|x| x.len() > 1)
            .map(|x| x.into_iter().map(|x| x.0).collect_vec())
            .collect_vec()
    }

    // TODO appeasing the borrow checker to use an iterator instead PERF
    pub fn nodes_by_type(&self, ty: &str) -> Vec<(EventIndex, Node)> {
        self.nodes().filter(|x| &self[x.1.ty] == ty).collect_vec()
    }

    // TODO PERF
    pub fn edges_by_type(&self, ty: &str) -> Vec<(EventIndex, Edge)> {
        self.edges().filter(|x| &self[x.1.ty] == ty).collect_vec()
    }

    pub fn basic_validation(&self) -> anyhow::Result<()> {
        // Non-Rules. Have little to do with the meaning of the graph, but more related to data integrity
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
                    anyhow::bail!("Not Every Event Has a Circumstance!");
                }
            }

            // Only a SINGLE Circumstance::Mock is allowed. This is only for running validation code before adding a new event
            // TODO modify this so that only TAIL Circumstance::Mock is allowed.
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
                        anyhow::bail!("A Circumstance::Mock has slip through the cracks");
                    }
                }
            }
        }

        // Rule: exact edge duplicate events are forbidden.
        // Rule: exact node duplicate events are forbidden if both are valid within a neighborhood; node duplicates are only discouraged otherwise
        {
            // Easy part: check edges
            let edge_count = self.edges().count();
            let unique_edge_count = self.edges().map(|x| x.1).unique().count();
            if edge_count != unique_edge_count {
                anyhow::bail!("Duplicate Edges!")
            }

            // Hard part: check nodes
            let dupe_class = self
                .nodes()
                .into_group_map_by(|x| x.1.clone())
                .into_values()
                .filter(|bucket| bucket.len() > 1)
                .collect_vec();

            for dupe in dupe_class {
                for pair in dupe.iter().combinations(2) {
                    let (older, newer) = {
                        if pair[0].0 < pair[1].0 {
                            (pair[0], pair[1])
                        } else {
                            (pair[1], pair[0])
                        }
                    };

                    if self.is_stale_by_and_including(older.0, newer.0) {
                        eprintln!(
                            "Duplicate nodes, but older one was invalidated later (still discouraged). EventIndex {} and {}",
                            older.0.0, newer.0.0
                        );
                    } else {
                        assert!({
                            // The check should pass even if an identical node overrides the parent
                            // What is the application of overriding a node with an exact copy?
                            // (Maybe counters, or means of reconsidering Stale Edges)
                            if let Some(idx) = newer.1.parent
                                && idx == older.0
                            {
                                false
                            } else {
                                true
                            }
                        });
                        anyhow::bail!(
                            "Exact Duplicate Nodes! EventIndex {} and {}",
                            older.0.0,
                            newer.0.0
                        )
                    }
                }
            }
        }

        // Rule: events are sorted, nodes and edges cannot refer to themselves or future
        // Rule: Edges can only be between nodes
        // Rule: Node parent can only be another node
        {
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
                                anyhow::bail!("Node {} has parent EventIndex out of range", idx.0);
                            }
                            if self[idx].is_edge() {
                                anyhow::bail!("Node {} has an edge parent", e.0);
                            }
                        }
                    }
                    Event::Edge(Edge { ty: _, from, to }) => {
                        if !idx_in_range(from) || !idx_in_range(to) {
                            anyhow::bail!("Edge {} has an end out of range", e.0);
                        }
                        if self[from].is_edge() || self[to].is_edge() {
                            anyhow::bail!("Edge {} has an end that is not a node", e.0);
                        }
                    }
                }
            }
        }

        // Rule: after a node has been overriden (that is, a new 'child' node has been added) it cannot be a part of a new edge. "Stale Rule"
        {
            for (edge_added_later, edge) in self.edges() {
                assert!(edge_added_later.0 > edge.from.0 && edge_added_later.0 > edge.to.0);
                if self.is_stale_by_and_including(edge.from, edge_added_later)
                    || self.is_stale_by_and_including(edge.to, edge_added_later)
                {
                    anyhow::bail!("Edge {} violates Stale Rule", edge_added_later.0)
                }
            }
        }

        anyhow::Result::Ok(())
    }
}
