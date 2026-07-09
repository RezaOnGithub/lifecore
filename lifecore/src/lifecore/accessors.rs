//! Accessor Zoo

use std::ops::Range;

use crate::{
    Circumstance, Edge, EdgeExt, Event, EventContext, EventIndex, LifeCore, Node, NodeExt,
    typecheck::NodeType,
};
use itertools::Itertools;

impl LifeCore {
    // Single Value Return
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
        self[i].as_raw_node()
    }

    pub(crate) fn raw_edge(&self, i: &EventIndex) -> Edge {
        self[i].as_raw_edge()
    }

    pub fn node(&self, ctx: EventContext) -> NodeExt {
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

    pub fn edge(&self, ctx: EventContext) -> EdgeExt {
        let raw_edge = self[ctx.thing()].as_raw_edge();
        EdgeExt {
            ty_name: self[raw_edge.ty].to_string(),
            from: ctx.look_at(raw_edge.from),
            to: ctx.look_at(raw_edge.to),
            e: ctx,
        }
    }

    pub fn root_of(&self, e: &EventIndex) -> Option<EventIndex> {
        if self[e].is_edge() {
            return None;
        }
        let mut r = *e;
        while let Some(parent_event) = self.raw_node(&r).parent {
            r = parent_event;
        }
        Some(r)
    }

    // Aggregate Return
    pub(crate) fn raw_nodes(
        &self,
        ctx: EventContext,
    ) -> impl Iterator<Item = (EventContext, Node)> {
        let w = || ctx.index_iter();
        w().zip(self.window(w()))
            .filter(|(_, event)| event.is_node())
            .map(|(index, event)| (index, event.as_raw_node()))
    }

    pub(crate) fn raw_edges(
        &self,
        ctx: EventContext,
    ) -> impl Iterator<Item = (EventContext, Edge)> {
        let w = || ctx.index_iter();
        w().zip(self.window(w()))
            .filter(|(_, event)| event.is_edge())
            .map(|(index, event)| (index, event.as_raw_edge()))
    }

    // NOTE retrieves nodes with their types resolved within the context. Convenient!
    // However! NOTE that the type may not actually be what was intended when the node was created
    pub fn nodes(&self, ctx: EventContext) -> impl Iterator<Item = (EventContext, NodeExt)> {
        let w = || ctx.index_iter();
        w().zip(self.window(w()))
            .filter(|(_, event)| event.is_node())
            .map(move |(index, _)| (index, self.node(ctx.look_at(index.thing()))))
    }

    pub fn edges(&self, ctx: EventContext) -> impl Iterator<Item = (EventContext, EdgeExt)> {
        let w = || ctx.index_iter();
        w().zip(self.window(w()))
            .filter(|(_, event)| event.is_edge())
            .map(move |(index, _)| (index, self.edge(ctx.look_at(index.thing()))))
    }

    // Return all the fresh types in context
    // Types are woven into the system at this point; special-cased accessors just make life easier
    pub fn types(
        &self,
        e: EventContext,
    ) -> impl Iterator<Item = (String, (EventContext, NodeType))> {
        // SUS does zipping index with the elements give a similar effect as `.enumerate()`? Are the indices correct?
        let w = || (&e).index_iter();
        w().zip(self.window(w()))
            .filter(move |(index, x)| {
                x.is_node()
                    && !self.is_stale(e.look_at(index.thing()))
                    && &self[x.as_raw_node().ty] == "Node_Type"
            })
            .map(|(index, x)| {
                let raw_node = x.as_raw_node();
                let body = self.resolve(&raw_node.payload);
                let ty = NodeType::try_from(&body).unwrap();
                (ty.name.clone(), (index, ty))
            })
    }

    // Return nodes that have a predecessor, with the predecessors
    pub fn non_solo_families(&self, ctx: EventContext) -> Vec<Vec<EventContext>> {
        self.raw_nodes(ctx)
            .filter(|(_, node)| node.parent.is_some())
            .into_group_map_by(|x| self.root_of(&x.0.thing()).unwrap())
            .into_values()
            .map(|children_pair| {
                // NOTE Does not contain the root
                assert!(!children_pair.is_empty());
                let mut children = children_pair
                    .into_iter()
                    .map(|(index, _)| index)
                    .collect_vec();
                children.push(ctx.look_at(self.root_of(&children[0].thing()).unwrap()));
                children
            })
            .collect_vec()
    }
}
