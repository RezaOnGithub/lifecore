//! Accessor Zoo

use itertools::Itertools;

use crate::{Edge, Event, EventIndex, LifeCore, Node};

impl LifeCore {
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
        range
            .into_iter()
            .map(|x| EventIndex(x as u64))
            .map(|x| (x, self[x].clone()))
            .filter(|(_, e)| e.is_node())
            .map(|(i, e)| (i, e.try_as_node().unwrap()))
    }

    pub fn edges_in_range(
        &self,
        range: std::ops::Range<usize>,
    ) -> impl Iterator<Item = (EventIndex, Edge)> {
        range
            .into_iter()
            .map(|x| EventIndex(x as u64))
            .map(|x| (x, self[x].clone()))
            .filter(|(_, e)| e.is_edge())
            .map(|(i, e)| (i, e.try_as_edge().unwrap()))
    }

    pub fn nodes_by_type(&self, ty: &str) -> impl Iterator<Item = (EventIndex, Node)> {
        self.nodes().filter(move |x| &self[x.1.ty] == ty)
    }

    pub fn edges_by_type(&self, ty: &str) -> impl Iterator<Item = (EventIndex, Edge)> {
        self.edges().filter(move |x| &self[x.1.ty] == ty)
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
            .into_iter()
            .filter(move |e| self.event[*e].is_edge() && &self[self.edge(e.into()).ty] == ty)
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
