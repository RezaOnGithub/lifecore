//! Implementation Details of Current LifeCoreList
use indexmap::IndexSet;
use itertools::Itertools;
use jiff::Unit;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt::Display, ops::Index, str::FromStr};
use strum::{EnumIs, EnumTryAs, VariantNames};
use whoami::{hostname, username};

use crate::{
    AnalysisConfig, AnalysisSuiteConfig, BasicValidationFailureScenario, EdgeExt, EventContext,
    LifeCore, LifeCoreError, NodeBodyExt, NodeExt, NodeType, PayloadExt, Timestamp, Zoned,
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
    // - nodes and edges that may get added as a part of the suggestions through AnalysisReport (may indicate "progression")
    configured_analyzer: AnalysisSuiteConfig,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Clone)]
struct NodeBody(HashMap<StringIndex, Payload>);

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
struct CircumstanceIndex(u64);

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
struct StringIndex(u64);

#[derive(Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
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
struct Edge {
    pub ty: StringIndex,
    pub from: EventIndex,
    pub to: EventIndex,
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
enum Payload {
    PreciseTime(Timestamp),
    Text(StringIndex),
    Number(i64),

    // NOTE this should be a last resort for users. As time goes on, we will expand
    // the list of primitve types to further minimize its use.
    // Serialized LOBs are discouraged.
    Serialized(StringIndex),
}

impl LifeCore for LifeCoreList {
    fn last(&self) -> EventContext {
        assert!(!self.event.is_empty());
        EventIndex((self.total_event_count() - 1) as u64).before_and_including()
    }

    fn total_event_count(&self) -> usize {
        self.event.len()
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

            let circumstance_index = t.get_or_insert_mock_circumstance().0;
            let ty_str_index = t.get_or_insert_string(ty);
            t.event.push(Event::Edge(Edge {
                ty: ty_str_index,
                from: from.thing(),
                to: to.thing(),
            }));
            t.circumstance.push(circumstance_index);
            let v = t.basic_validation();
            if !v.is_success() {
                return Err(v.err());
            }
            let _ = Ok(self.edge(self.last()));
        }

        // NOTE COPY_PASTE
        // Circumstance MUST come first, because if it fails somehow, then we inserted intermediate data for nothing
        let current_circumstance_index = self.get_or_insert_current_session_circumstance()?.0;
        let ty_str_index = self.get_or_insert_string(ty);
        self.event.push(Event::Edge(Edge {
            ty: ty_str_index,
            from: from.into(),
            to: to.into(),
        }));
        self.circumstance.push(current_circumstance_index);
        assert!(self.basic_validation().is_success());
        Ok(self.edge(self.last()))
    }

    fn add(
        &mut self,
        ty: &str,
        payload: NodeBodyExt,
        parent: Option<EventContext>,
    ) -> Result<NodeExt, LifeCoreError> {
        let insert_all_strings = |t: &mut Self, payload: &NodeBodyExt| -> NodeBody {
            let mut r = NodeBody::new();
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
            let circumstance_index = t.get_or_insert_mock_circumstance().0;
            let ty_str_index = t.get_or_insert_string(ty);
            let p = insert_all_strings(&mut t, &payload);
            t.event.push(Event::Node(Node {
                ty: ty_str_index,
                parent,
                payload: p,
            }));
            t.circumstance.push(circumstance_index);
            t.basic_validation()?;
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

    fn get_or_insert_current_session_circumstance(
        &mut self,
    ) -> Result<crate::CircumstanceExt, LifeCoreError> {
        todo!()
    }

    fn get_or_insert_mock_circumstance(&mut self) -> Result<crate::CircumstanceExt, LifeCoreError> {
        todo!()
    }

    fn get(&self, e: EventContext) -> crate::EventExt {
        
    }

    fn window<I>(&self, i: I) -> impl Iterator<Item = &crate::EventExt>
    where
        I: IntoIterator<Item = EventContext>,
    {
        i.into_iter().map_while(|x| self.event.get(x.thing().i()))
    }

    fn is_stale(&self, e: EventContext) -> bool {
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
            if self.is_stale(e.look_at(from.into())) || self.is_stale(e.look_at(to)) {
                return true;
            }
        }
        }
        false
    }

    fn basic_validation(&self) -> crate::AnalysisOutput {
        let mut r = crate::AnalysisOutput(vec![]);
        macro_rules! warn {
            (
                $e0:expr,
                $($x:expr),+
            ) => {
                r.0.push(crate::AnalysisReport::BasicValidationWarning {
                    offending_event: $e0,
                    message: format!($($x),+),
                })
            };
        }
        macro_rules! err {
            (
                $e0:expr,
                $x:expr
            ) => {
                r.0.push(crate::AnalysisReport::BasicValidationFailure {
                    offending_event: $e0,
                    reason: $x,
                })
            };
        }
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
                    err!(
                        EventContext::universe(),
                        BasicValidationFailureScenario::InternalError {
                            message: format!("Circumstance list does not match up with Event list")
                        }
                    );
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
                        // TODO better context
                        err!(
                            EventContext::universe(),
                            BasicValidationFailureScenario::ExtraneousMockCircumstance
                        );
                    }
                }
            }
        }

        // Rule: exact edge duplicate events are forbidden.
        // Rule: exact node duplicate events are forbidden if both are valid within a neighborhood; node duplicates are _only discouraged_ otherwise
        {
            // Easy part: check edges
            let edge_count = self.edges(ctx()).count();
            let unique_edge_count = self.raw_edges(ctx()).map(|x| x.1).unique().count();
            if edge_count != unique_edge_count {
                // TODO not so easy now; give better context
                err!(
                    EventContext::universe(),
                    BasicValidationFailureScenario::DuplicateEdge
                );
            }

            // Hard part: check nodes
            let dupe_class = self
                .raw_nodes(ctx())
                .into_group_map_by(|x| x.1)
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

                    if self.is_stale(
                        newer_index
                            .exclusive_left_context()
                            .look_at(*older_index),
                    ) {
                        warn!(
                            *newer_index,
                            "Duplicate nodes, but older one was invalidated before the new one was added (discouraged). EventIndex {} and {}",
                            older_index,
                            newer_index
                        );
                    } else {
                        assert!({
                            // The check above should pass if an identical node overrides the parent
                            // What is the application of overriding a node with an exact copy?
                            // (Maybe counters, or means of correcting edges or getting rid of stale edges)
                            if let Some(idx) = newer_node.parent
                                && idx == older_index.thing()
                            {
                                false
                            } else {
                                true
                            }
                        });
                        err!(*newer_index, BasicValidationFailureScenario::DuplicateNode)
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
                            err!(
                                EventContext::universe().look_at(idx.into()),
                                BasicValidationFailureScenario::InternalError {
                                    message: format!(
                                        "Node {} has parent EventIndex out of range",
                                        idx.0
                                    )
                                }
                            );
                        }
                        if self[idx].is_edge() {
                            err!(
                                EventContext::universe().look_at(idx.into()),
                                BasicValidationFailureScenario::InternalError {
                                    message: format!("Node {} has an edge parent", e.0)
                                }
                            );
                        }
                    }
                }
                Event::Edge(Edge { ty: _, from, to }) => {
                    if !idx_in_range(from) || !idx_in_range(to) {
                        err!(
                            EventContext::universe().look_at(EventIndex::from(e.0)),
                            BasicValidationFailureScenario::InternalError {
                                message: format!("Edge {} has an end out of range", e.0)
                            }
                        );
                    }
                    if self[from].is_edge() || self[to].is_edge() {
                        err!(
                            EventContext::universe().look_at(EventIndex::from(e.0)),
                            BasicValidationFailureScenario::InternalError {
                                message: format!("Edge {} has an end that is not a node", e.0)
                            }
                        );
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
                    err!(
                        edge_context,
                        BasicValidationFailureScenario::StaleRuleViolation
                    );
                }
            }
        }

        r
    }

    fn node(&self, ctx: EventContext) -> NodeExt {
        let raw_node = self[ctx.thing()].as_raw_node();
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
        NodeExt {
            ty,
            ty_name: self[raw_node.ty].to_string(),
            field: body,
            parent: raw_node.parent,
            e: ctx,
        }
    }

    fn edge(&self, ctx: EventContext) -> EdgeExt {
        let raw_edge = self[ctx.thing()].as_raw_edge();
        EdgeExt {
            ty_name: self[raw_edge.ty].to_string(),
            from: ctx.look_at(raw_edge.from),
            to: ctx.look_at(raw_edge.to),
            e: ctx,
        }
    }

    fn nodes(&self, ctx: EventContext) -> impl Iterator<Item = (EventContext, NodeExt)> {
        let w = || ctx.index_iter();
        w().zip(self.window(w()))
            .filter(|(_, event)| event.is_node())
            .map(move |(index, _)| (index, self.node(ctx.look_at(index))))
    }

    fn edges(&self, ctx: EventContext) -> impl Iterator<Item = (EventContext, EdgeExt)> {
        let w = || ctx.index_iter();
        w().zip(self.window(w()))
            .filter(|(_, event)| event.is_edge())
            .map(move |(index, _)| (index, self.edge(ctx.look_at(index))))
    }

    fn root_of(&self, e: EventContext) -> Option<EventContext> {
        if self[e.thing()].is_edge() {
            return None;
        }
        let mut r = e.thing();
        while let Some(parent_event) = self.raw_node(&r).parent {
            r = parent_event;
        }
        Some(r.before_and_including())
    }

    fn types(&self, e: EventContext) -> impl Iterator<Item = (String, (EventContext, NodeType))> {
        let w = || e.index_iter();
        w().zip(self.window(w()))
            .filter(move |(index, x)| {
                x.is_node()
                    && !self.is_stale(e.look_at(*index))
                    && &self[x.as_raw_node().ty] == "Node_Type"
            })
            .map(|(index, x)| {
                let raw_node = x.as_raw_node();
                let body = self.resolve(&raw_node.payload);
                let ty = NodeType::try_from(&body).unwrap();
                (ty.name.clone(), (index, ty))
            })
    }

    fn non_solo_families(&self, ctx: EventContext) -> Vec<Vec<EventContext>> {
        self.raw_nodes(ctx)
            .filter(|(_, node)| node.parent.is_some())
            .into_group_map_by(|x| self.root_of(x.0).unwrap())
            .into_values()
            .map(|children_pair| {
                // NOTE Does not contain the root
                assert!(!children_pair.is_empty());
                let mut children = children_pair
                    .into_iter()
                    .map(|(index, _)| index)
                    .collect_vec();
                children.push(ctx.look_at(self.root_of(children[0]).unwrap()));
                children
            })
            .collect_vec()
    }

    fn add_analyzer(&mut self, name: &str, config: &str) {
        self.configured_analyzer.0.push(AnalysisConfig {
            name: name.to_string(),
            config: config.to_string(),
        });
    }

    fn current_analysis_suite(&self) -> AnalysisSuiteConfig {
        self.configured_analyzer.clone()
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
    fn new() -> Self {
        Self {
            event: vec![],
            circumstance: vec![],
            circumstance_table: IndexSet::new(),
            string_table: IndexSet::new(),
            configured_analyzer: AnalysisSuiteConfig(vec![]),
        }
    }

    fn resolve(&self, t: &NodeBody) -> NodeBodyExt {
        let mut r = HashMap::<String, PayloadExt>::new();
        for (k, v) in t.0.iter() {
            r.insert(self[k].to_string(), {
                match v {
                Payload::PreciseTime(timestamp) => PayloadExt::PreciseTime(*timestamp),
                Payload::Text(string_index) => PayloadExt::Text(self[string_index].to_string()),
                Payload::Number(i) => PayloadExt::Number(*i),
                Payload::Serialized(string_index) => PayloadExt::Serialized(
                    serde_json::Value::from_str(&self[string_index]).unwrap(),
                ),
                }
            });
        }
        NodeBodyExt(r)
    }

    fn get_or_insert_current_session_circumstance(
        &mut self,
    ) -> Result<(CircumstanceIndex, Circumstance), LifeCoreError> {
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
                self.circumstance_table.insert(circumstance.clone()),
                "Circumstance should not be present. We just checked!"
            );
            anyhow::Result::Ok((
                CircumstanceIndex((self.circumstance_table.len() - 1) as u64),
                circumstance,
            ))
        }
    }

    fn get_or_insert_mock_circumstance(&mut self) -> (CircumstanceIndex, Circumstance) {
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

    // Aggregate Return
    fn raw_nodes(&self, ctx: EventContext) -> impl Iterator<Item = (EventContext, Node)> {
        let w = || ctx.index_iter();
        w().zip(self.window(w()))
            .filter(|(_, event)| event.is_node())
            .map(|(index, event)| (index, event.as_raw_node()))
    }

    fn raw_edges(&self, ctx: EventContext) -> impl Iterator<Item = (EventContext, Edge)> {
        let w = || ctx.index_iter();
        w().zip(self.window(w()))
            .filter(|(_, event)| event.is_edge())
            .map(|(index, event)| (index, event.as_raw_edge()))
    }
}

impl EventIndex {
    fn before(&self) -> EventContext {
        EventContext::from(0..(self.0 as usize), None)
    }

    fn before_and_including(&self) -> EventContext {
        // Can't use RangeInclusive because it can't be converted to Range easily
        EventContext::from(0..((self.0 + 1) as usize), Some(self.0 as usize))
    }

    fn i(self) -> usize {
        self.0 as usize
    }
}

impl From<EventContext> for EventIndex {
    fn from(value: EventContext) -> Self {
        assert!(value.validate());
        Self(u64::try_from(value.thing.unwrap()).unwrap())
    }
}

impl From<&EventContext> for EventIndex {
    fn from(value: &EventContext) -> Self {
        Self::from(*value)
    }
}

impl From<EventIndex> for EventContext {
    fn from(value: EventIndex) -> Self {
        Self::universe().look_raw(value.i())
    }
}

impl From<&EventIndex> for EventContext {
    fn from(value: &EventIndex) -> Self {
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

impl EventContext {
    fn thing(&self) -> EventIndex {
        assert!(self.validate() && self.thing.is_some());
        EventIndex::from(self.thing.unwrap())
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

impl NodeBody {
    fn new() -> Self {
        Self(HashMap::new())
    }

    fn insert(&mut self, key: StringIndex, val: Payload) {
        assert!(!self.0.contains_key(&key));
        assert!(self.0.insert(key, val).is_none());
    }

    fn from(tab: HashMap<StringIndex, Payload>) -> Self {
        assert!(tab.len() <= 32, "HashMap too large!");
        let mut r = Self::new();
        for x in tab.into_iter() {
            r.insert(x.0, x.1);
        }
        r
    }
}

impl std::hash::Hash for NodeBody {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let mut x = self.0.iter().collect_vec();
        x.sort_by(|a, b| a.0.cmp(b.0));
        x.hash(state);
    }
}
