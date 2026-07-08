//! Accessor Zoo

use std::ops::Range;

use crate::{
    Circumstance, Edge, EdgeExt, Event, EventIndex, LifeCore, Node, NodeExt, typecheck::NodeType,
};
use itertools::Itertools;

impl LifeCore {
    // Helper accessors. Returning Node, Edge, and more internal types
    pub(crate) fn get(&self, i: &EventIndex) -> (Circumstance, Event) {
        (
            self.circumstance_table
                .get_index(self.circumstance[i.0 as usize].0 as usize)
                .expect("CirucmstanceIndex out of range")
                .clone(),
            self[i].clone(),
        )
    }

    pub(crate) fn raw_node(&self, i: &EventIndex) -> Node {
        self[i].clone().try_as_node().unwrap()
    }

    pub(crate) fn raw_edge(&self, i: &EventIndex) -> Edge {
        self[i].clone().try_as_edge().unwrap()
    }

    pub(crate) fn raw_nodes(&self) -> impl Iterator<Item = (EventIndex, Node)> {
        self.event
            .iter()
            .enumerate()
            .filter(|x| x.1.is_node())
            .map(|x| (x.0.into(), x.1.clone().try_as_node().unwrap()))
    }

    pub(crate) fn raw_edges(&self) -> impl Iterator<Item = (EventIndex, Edge)> {
        self.event
            .iter()
            .enumerate()
            .filter(|x| x.1.is_edge())
            .map(|x| (x.0.into(), x.1.clone().try_as_edge().unwrap()))
    }

    // Public Interface. Returning NodeExt and EdgeExt
    pub fn node_in_context(&self, e: EventIndex, window: Range<usize>) -> NodeExt {
        let raw_node = self[e].clone().try_as_node().unwrap();
        let ty = {
            if let Some((_, (_, ty))) = self
                .types_in_window(e.before_and_including())
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
        }
    }

    pub fn edge(&self, e: EventIndex) -> EdgeExt {
        let raw_edge = self[e].clone().try_as_edge().unwrap();
        EdgeExt {
            ty_name: self[raw_edge.ty].to_string(),
            from: raw_edge.from,
            to: raw_edge.to,
        }
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

    pub fn edges(&self) -> impl Iterator<Item = (EventIndex, EdgeExt)> {
        self.event
            .iter()
            .enumerate()
            .filter(|x| x.1.is_edge())
            .map(|x| (x.0.into(), self.edge(x.0.into())))
    }

    pub fn nodes(&self) -> impl Iterator<Item = (EventIndex, NodeExt)> {
        self.event
            .iter()
            .enumerate()
            .filter(|x| x.1.is_node())
            .map(|x| (x.0.into(), self.node(x.0.into())))
    }

    pub fn nodes_in_range(
        &self,
        range: std::ops::Range<usize>,
    ) -> impl Iterator<Item = (EventIndex, NodeExt)> {
        range
            .into_iter()
            .map(|x| EventIndex(x as u64))
            .map(|x| (x, self[x].clone()))
            .filter(|(_, e)| e.is_node())
            .map(|(i, _)| (i, self.node(i)))
    }

    pub fn edges_in_range(
        &self,
        range: std::ops::Range<usize>,
    ) -> impl Iterator<Item = (EventIndex, EdgeExt)> {
        range
            .into_iter()
            .map(|x| EventIndex(x as u64))
            .map(|x| (x, self[x].clone()))
            .filter(|(_, e)| e.is_edge())
            .map(|(i, _)| (i, self.edge(i)))
    }

    pub fn nodes_by_type(&self, ty: &str) -> impl Iterator<Item = (EventIndex, NodeExt)> {
        self.nodes().filter(move |x| x.1.ty_name.as_str() == ty)
    }

    pub fn edges_by_type(&self, ty: &str) -> impl Iterator<Item = (EventIndex, EdgeExt)> {
        self.edges().filter(move |x| x.1.ty_name.as_str() == ty)
    }

    pub fn fresh_nodes_in_range(
        &self,
        range: std::ops::Range<usize>,
    ) -> impl Iterator<Item = (EventIndex, NodeExt)> {
        self.nodes_in_range(range.clone()).filter(move |(e, _)| {
            !(range.clone().count() >= 1
                && self
                    .is_stale_by_and_including(*e, EventIndex::from(range.clone().last().unwrap())))
        })
    }

    pub fn fresh_edges_in_range(
        &self,
        range: std::ops::Range<usize>,
    ) -> impl Iterator<Item = (EventIndex, EdgeExt)> {
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
    ) -> impl Iterator<Item = (EventIndex, NodeExt)> {
        range
            .into_iter()
            .filter(move |e| self.event[*e].is_node() && self.node(e.into()).ty_name.as_str() == ty)
            .map(|x| (x.into(), self.node(x.into())))
    }

    pub fn edges_in_range_of_type(
        &self,
        ty: &str,
        range: std::ops::Range<usize>,
    ) -> impl Iterator<Item = (EventIndex, EdgeExt)> {
        range
            .into_iter()
            .filter(move |e| self.event[*e].is_edge() && self.edge(e.into()).ty_name.as_str() == ty)
            .map(|x| (x.into(), self.edge(x.into())))
    }

    pub fn fresh_events_in_range(
        &self,
        range: std::ops::Range<usize>,
    ) -> impl Iterator<Item = (EventIndex, Event)> {
        range
            .clone()
            .into_iter()
            .map(|x| EventIndex(x as u64))
            .filter(move |x| {
                !self
                    .is_stale_by_and_including(*x, EventIndex(range.clone().last().unwrap() as u64))
            })
            .map(|x| (x, self[x].clone()))
    }

    pub fn fresh_nodes_in_range_of_type(
        &self,
        ty: &str,
        range: std::ops::Range<usize>,
    ) -> impl Iterator<Item = (EventIndex, NodeExt)> {
        range
            .clone()
            .into_iter()
            .filter(move |e| {
                self.event[*e].is_node()
                    && self.node(e.into()).ty_name.as_str() == ty
                    && !self.is_stale_by_and_including(e.into(), (range.end - 1).into())
            })
            .map(|x| (x.into(), self.node(x.into())))
    }

    pub fn fresh_edges_in_range_of_type(
        &self,
        ty: &str,
        range: std::ops::Range<usize>,
    ) -> impl Iterator<Item = (EventIndex, EdgeExt)> {
        // NOTE if range.count() >= 1 then (range.end - 1) is always valid
        range
            .clone()
            .into_iter()
            .filter(move |e| {
                self.event[*e].is_edge()
                    && self.edge(e.into()).ty_name.as_str() == ty
                    && !self.is_stale_by_and_including(e.into(), (range.end - 1).into())
            })
            .map(|x| (x.into(), self.edge(x.into())))
    }

    // Return all the fresh types in 0..e
    // Types are woven into the system at this point; special-cased accessors just make life easier
    pub fn types_in_window(
        &self,
        window: Range<usize>,
    ) -> impl Iterator<Item = (String, (EventIndex, NodeType))> {
        assert!(window.start == 0, "Not Well Understood");
        let last = {
            if let Some(x) = window.clone().last() {
                Some(EventIndex::from(x))
            } else {
                None
            }
        };
        window
            .clone()
            .into_iter()
            .filter(move |e| {
                self.event[*e].is_node()
                    && !self.is_stale_by_and_including(e.into(), last.unwrap())
                    && &self[self.event[*e].clone().try_as_node().unwrap().ty] == "Node_Type"
            })
            .map(|e| {
                let raw_node = self[EventIndex::from(e)].clone().try_as_node().unwrap();
                let body = self.resolve(&raw_node.payload);
                let ty = NodeType::try_from(&body).unwrap();
                (ty.name.clone(), (EventIndex::from(e), ty))
            })
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
}
