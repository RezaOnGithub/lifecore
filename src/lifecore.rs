use indexmap::IndexSet;
use itertools::Itertools;
use jiff::{Unit, Zoned};
use std::{collections::HashMap, ops::Index};
use strum::VariantNames;
use whoami::{hostname, username};

use crate::{
    Circumstance, CircumstanceIndex, Edge, Event, EventIndex, LifeCore, Node, Payload, PayloadExt, SmallTable, StringIndex, analyzer::AnalysisSuite, to_map
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
        let mut r = Self {
            event: vec![],
            circumstance: vec![],
            circumstance_table: IndexSet::new(),
            string_table: IndexSet::new(),
            analyzer: AnalysisSuite(vec![])
        };
        r.add(
            "Node_Type",
            to_map(&[("Type_Name", PayloadExt::Text("Node_Type".to_string()))]),
            None,
        )
        .unwrap();
        r
    }

    pub fn upper(&self) -> EventIndex {
        assert!(!self.event.is_empty());
        EventIndex((self.event.len() - 1) as u64)
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
        Ok(self.upper())
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
        Ok(self.upper())
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

    pub fn edges(&self) -> impl Iterator<Item = (EventIndex, Edge)> {
        self.event
            .iter()
            .enumerate()
            .filter(|x| x.1.is_edge())
            .map(|x| (EventIndex(x.0 as u64), x.1.clone().try_as_edge().unwrap()))
    }

    pub fn nodes(&self) -> impl Iterator<Item = (EventIndex, Node)> {
        self.event
            .iter()
            .enumerate()
            .filter(|x| x.1.is_node())
            .map(|x| (EventIndex(x.0 as u64), x.1.clone().try_as_node().unwrap()))
    }

    pub fn total_event_count(&self) -> usize {
        self.event.len()
    }

    // Get nodes in the first `count` events
    pub fn nodes_up_to(&self, count: usize) -> impl Iterator<Item = (EventIndex, Node)> {
        // NOTE if we attempt to implement it like this:
        // ```
        //      self.event.iter().enumerate().filter(|(i,x)| ...)
        // ```
        // You will get a lifetime error because the closure now has to keep a reference to `count`
        // This way it's simpler.
        self.event[..count]
            .iter()
            .enumerate()
            .filter(|x| x.1.is_node())
            .map(|x| (EventIndex(x.0 as u64), x.1.clone().try_as_node().unwrap()))
    }

    // Get edges in the first `count` events
    pub fn edges_up_to(&self, count: usize) -> impl Iterator<Item = (EventIndex, Edge)> {
        self.event[..count]
            .iter()
            .enumerate()
            .filter(|x| x.1.is_edge())
            .map(|x| (EventIndex(x.0 as u64), x.1.clone().try_as_edge().unwrap()))
    }

    // Check whether a `e` has become stale within `0..upper+1` (upper inclusive)
    // Stale Node means a newer node has it as its parent
    // Stale Edge means at least one end is a Stale Node
    // NOTE in Stale Node case, upper could refer to the child that makes `e` stale (parent of upper == e)
    // TODO check if the above fact is considered where this is called
    pub fn is_stale_by(&self, e: EventIndex, upper: EventIndex) -> bool {
        match self[e] {
            Event::Node(_) => {
                let window = &self.event[(e.0 as usize)..(upper.0 + 1) as usize];
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
                if self.is_stale_by(from, upper) || self.is_stale_by(to, upper) {
                    return true;
                }
            }
        }
        false
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

                    if self.is_stale_by(older.0, newer.0) {
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
            for (idx, edge) in self.edges() {
                if self.is_stale_by(edge.from, idx) || self.is_stale_by(edge.to, idx) {
                    anyhow::bail!("Edge {} violates Stale Rule", idx.0)
                }
            }
        }

        anyhow::Result::Ok(())
    }
}
