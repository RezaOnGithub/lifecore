use indexmap::IndexSet;
use itertools::Itertools;
use jiff::{Unit, Zoned};
use std::{collections::HashMap, ops::Index, str::FromStr};
use strum::VariantNames;
use whoami::{hostname, username};

use crate::{
    Circumstance, CircumstanceIndex, Edge, Event, EventContext, EventIndex, LifeCore, Node,
    NodeBody, NodeBodyExt, Payload, PayloadExt, StringIndex,
    analysis::{AnalysisConfig, AnalysisSuiteConfig},
};

mod accessors;

impl LifeCore {
    pub fn new() -> Self {
        Self {
            event: vec![],
            circumstance: vec![],
            circumstance_table: IndexSet::new(),
            string_table: IndexSet::new(),
            configured_analyzer: AnalysisSuiteConfig(vec![]),
        }
    }

    pub fn add_analyzer(&mut self, name: &str, config: &str) {
        self.configured_analyzer.0.push(AnalysisConfig {
            name: name.to_string(),
            config: config.to_string(),
        });
    }

    // get the INDEX for the last event
    pub fn last(&self) -> EventIndex {
        assert!(!self.event.is_empty());
        EventIndex((self.total_event_count() - 1) as u64)
    }

    // get the COUNT of all events
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
        payload: NodeBodyExt,
        parent: Option<EventIndex>,
    ) -> anyhow::Result<EventIndex> {
        let insert_all_strings = |t: &mut Self, payload: &NodeBodyExt| -> NodeBody {
            let mut r = NodeBody::new();
            for (k, v) in payload.0.iter() {
                let smalltable_key = t.get_or_insert_string(&k);
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

    pub(crate) fn resolve(&self, t: &NodeBody) -> NodeBodyExt {
        let mut r = HashMap::<String, PayloadExt>::new();
        for (k, v) in t.0.iter().filter(|x| x.is_some()).map(|x| x.unwrap()) {
            r.insert(self[k].to_string(), {
                match v {
                Payload::PreciseTime(timestamp) => PayloadExt::PreciseTime(timestamp),
                Payload::Text(string_index) => PayloadExt::Text(self[string_index].to_string()),
                Payload::Number(i) => PayloadExt::Number(i),
                Payload::Serialized(string_index) => PayloadExt::Serialized(
                    serde_json::Value::from_str(&self[string_index]).unwrap(),
                ),
                }
            });
        }
        NodeBodyExt(r)
    }

    pub(crate) fn get_or_insert_current_session_circumstance(
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
            assert!(
                !self.circumstance_table.insert(circumstance.clone()),
                "Circumstance should not be present. We just checked!"
            );
            anyhow::Result::Ok((
                CircumstanceIndex((self.circumstance_table.len() - 1) as u64),
                circumstance,
            ))
        }
    }

    pub(crate) fn get_or_insert_mock_circumstance(&mut self) -> (CircumstanceIndex, Circumstance) {
        if let Some(idx) = self.circumstance_table.get_index_of(&Circumstance::Mock) {
            (CircumstanceIndex(idx as u64), Circumstance::Mock)
        } else {
            assert!(
                !self.circumstance_table.insert(Circumstance::Mock),
                "Circumstance should not be present. We just checked!"
            );
            (
                CircumstanceIndex((self.circumstance_table.len() - 1) as u64),
                Circumstance::Mock,
            )
        }
    }

    pub(crate) fn get_or_insert_string(&mut self, s: &str) -> StringIndex {
        let x = self.string_table.get_index_of(s);
        if x.is_some() {
            StringIndex(x.unwrap() as u64)
        } else {
            assert!(!self.string_table.insert(s.to_string()));
            StringIndex((self.string_table.len() - 1) as u64)
        }
    }

    pub(crate) fn window<I>(&self, i: I) -> impl Iterator<Item = &Event>
    where
        I: IntoIterator<Item = EventContext>,
    {
        i.into_iter().map_while(|x| self.event.get(x.thing().i()))
    }

    // Check whether `e.thing` is stale within context
    // Stale Node means a newer node has it as its parent within context
    // Stale Edge means at least one end is a Stale Node
    pub fn is_stale(&self, e: EventContext) -> bool {
        match self[e.thing()] {
        Event::Node(_) => {
            for potential_child in self
                .window(e.exclusive_right())
                .filter(|x| x.is_node())
                .map(|x| x.as_raw_node())
            {
                if let Some(idx) = potential_child.parent
                    && idx == e.thing()
                {
                    return true;
                }
            }
        }
        Event::Edge(Edge { ty: _, from, to }) => {
            // SUS
            if self.is_stale(e.look_at(from)) || self.is_stale(e.look_at(to)) {
                return true;
            }
        }
        }
        false
    }

    // TODO update this to the new AnalysisOutput system
    pub fn basic_validation(&self) -> anyhow::Result<()> {
        // Applies to all events, past and present.
        // NOTDO PERF
        let ctx = || EventContext::universe();

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
            // TODO modify this so that only TAIL Circumstance::Mock are allowed.
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
        // Rule: exact node duplicate events are forbidden if both are valid within a neighborhood; node duplicates are _only discouraged_ otherwise
        {
            // Easy part: check edges
            // TODO not so easy! Give a proper error message as to what duplicate edges were found
            let edge_count = self.edges(ctx()).count();
            let unique_edge_count = self.raw_edges(ctx()).map(|x| x.1).unique().count();
            if edge_count != unique_edge_count {
                anyhow::bail!("Duplicate Edges!")
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
                        if pair[0].0 < pair[1].0 {
                            (pair[0], pair[1])
                        } else {
                            (pair[1], pair[0])
                        }
                    };

                    if self.is_stale(newer_index.before().look_at(*older_index)) {
                        eprintln!(
                            "Duplicate nodes, but older one was invalidated before the new one was added (discouraged). EventIndex {} and {}",
                            older_index.0, newer_index.0
                        );
                    } else {
                        assert!({
                            // The check above should pass if an identical node overrides the parent
                            // What is the application of overriding a node with an exact copy?
                            // (Maybe counters, or means of correcting edges or getting rid of stale edges)
                            if let Some(idx) = newer_node.parent
                                && idx == *older_index
                            {
                                false
                            } else {
                                true
                            }
                        });
                        anyhow::bail!(
                            "Exact Duplicate Nodes! EventIndex {} and {}",
                            older_index.0,
                            newer_index.0
                        )
                    }
                }
            }
        }

        // Rule: events are sorted, nodes and edges cannot refer to themselves or the future
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
            for (edge_context, edge) in self.raw_edges(ctx()) {
                // Throw away the loop context. Now we are looking at 0..edge_index
                // (inclusive or exclusive doesn't matter in this particular case)
                let ctx = edge_context.thing().before();
                assert!(edge_context.thing().0 > edge.from.0 && edge_context.thing().0 > edge.to.0);
                if self.is_stale(ctx.look_at(edge.from)) || self.is_stale(ctx.look_at(edge.to)) {
                    anyhow::bail!("Edge {} violates Stale Rule", edge_context.thing())
                }
            }
        }

        anyhow::Result::Ok(())
    }
}

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
