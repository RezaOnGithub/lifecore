//! "Type System" Constraint System
//!
//! This implements a basic type checking system that is compatible with the limited VCS capabilities.
//! A "Node_Type" declares a new type with a unique name among non-stale nodes, the edges that are
//! allowed to connect to or from the node, and a node class: Toothpick (only a single edge that is
//! outgoing), Hub (only incoming edges), Spike (only outgoing edges), Regular (no restrictions) EdgeNode
//! (exactly one incoming and exactly one outgoing)
//!
//! The general idea is: at every event, all node must get typechecked against exactly one non-stale Node_Type
//! that is older or the same age; That is, if a node type X is overriden at E, all non-stale X nodes by E must
//! typecheck against the new type X' that overrode X.
//!
//! This can be terribly inefficient, and of course there is a streaming approach but correctness for now. NOTDO PERF

use crate::internal::PayloadType;
use crate::{analysis::*, internal::EventIndex};
use crate::*;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[cfg(test)]
mod test;

const TYPECHECK_NAME: &str = "Type_Check";

pub struct TypeCheckFactory;
impl AnalyzerFactory for TypeCheckFactory {
    fn analyzer_with_config(&mut self, config: &str) -> Result<Box<dyn Analyzer>, LifeCoreError> {
        Ok(Box::new(TypeCheck {
            config: config.to_string(),
        }))
    }

    fn analyzer_name(&self) -> &'static str {
        TYPECHECK_NAME
    }
}

pub struct TypeCheck {
    pub config: String,
}

impl Analyzer for TypeCheck {
    // Due to the very strict and picky system I've come up with, the very first event MUST be a node that
    // declares that there is gonna be nodes that specify types; that is, a node of type "Node_Type" that
    // declares "Node_Type". This bootstrap process needs to happen, and a non-stale node of type "Node_Type"
    // declaring "Node_Type" must always exist at any EventIndex
    // Additionally, this should strongly suggest some of the essential node types to add. Currently, Tombstone is very
    // important, but maybe in the future we need more special-cased node types
    fn initialize(&mut self, _lc: &LifeCore) -> AnalysisOutput {
        AnalysisOutput(vec![])
    }

    fn analyze(&self, lc: &LifeCore) -> AnalysisOutput {
        let mut r = AnalysisOutput(vec![]);
        macro_rules! warn {
            (
                $e0:expr,
                $($x:expr),+
                $(; $($y:expr),* $(,)?)?
                $(,)?
            ) => {
                r.0.push(AnalysisReport::Warning(ConstraintSystemMessage {
                    offending_event: $e0,
                    system_name: TYPECHECK_NAME.to_string(),
                    system_config: self.config.clone(),
                    relevance: vec![ $($($y),*)? ],
                    message: format!($($x),+),
                }));
            };
        }
        macro_rules! err {
            (
                $e0:expr,
                $($x:expr),+
            ) => {
                r.0.push(AnalysisReport::Inconsistency(ConstraintSystemMessage {
                    offending_event: $e0,
                    system_name: TYPECHECK_NAME.to_string(),
                    system_config: self.config.clone(),
                    relevance: vec![],
                    message: format!($($x),+),
                }));
            };
        }

        // check whether a Node_Type declaring some type X is not clashing with an older but non-stale
        // Node_Type declaring a type of the same name.
        for (node_type_index, node_type_raw) in lc
            .raw_nodes(EventContext::universe())
            .filter(|(_, node)| &lc[node.ty] == NodeType::name())
        {
            let type_table = lc
                .types(node_type_index.exclusive_left_context())
                .collect::<HashMap<_, _>>();
            let node_type = NodeType::try_from(&lc.resolve(&node_type_raw.payload)).unwrap();
            if let Some((older_type_index, _older_type)) = type_table.get(node_type.name.as_str()) {
                // The only scenario this is valid is if the new node override the old one
                if node_type_raw
                    .parent
                    .is_none_or(|x| x != older_type_index.thing())
                {
                    err!(
                        node_type_index
                            .inclusive_left_context()
                            .look_at(node_type_index.thing()),
                        "Cannot have a Node_Type declaring {} which was already declared",
                        node_type.name.as_str()
                    );
                }
            }
        }

        // Check overrides in types
        for last in lc.last().inclusive_left() {
            let ctx = last.inclusive_left_context();
            let type_table = lc.types(ctx).collect::<HashMap<_, _>>();
            // Check that override types exist
            // TODO I'm not sure whether this should be an error or not.
            // What it's causing at the moment is that interrelated Node_Types have to complete
            // a ritual to allow overriding each othe. I'm demoting this to a warning for now.
            for (ty_event, ty) in type_table.values() {
                for override_ty in ty.additional_override.iter() {
                    if !type_table.contains_key(override_ty) {
                        warn!(
                            *ty_event,
                            "Override {} on type {} does not (yet) exist",
                            override_ty,
                            ty.name.as_str();
                            *ty_event
                        );
                    }
                }
            }
        }

        // Check nodes for their fields
        for last in 0..lc.total_event_count() {
            let ctx = EventIndex::from(last).before_and_including();
            for pointed_context in ctx
                .index_iter()
                .filter(|e| !lc.is_stale(*e) && lc[e.thing()].is_node())
            {
                let node = lc.node(pointed_context);
                if node.ty.is_none() {
                    err!(
                        pointed_context,
                        "Type {} does not exist",
                        node.ty_name.as_str()
                    );
                    continue;
                }
                let node_type = node.ty.unwrap();

                // Fields that may be present and mandatory
                for (field_name, (field_type, field_policy)) in node_type.field.iter() {
                    match field_policy {
                    NodeTypeFieldStatus::Mandatory => {
                        if !node.field.0.contains_key(field_name) {
                            err!(
                                pointed_context,
                                "Node of type {} does not have the mandatory field {}",
                                node_type.name.as_str(),
                                field_name
                            );
                        }
                    }
                    NodeTypeFieldStatus::Encouraged => {
                        if !node.field.0.contains_key(field_name) {
                            warn!(
                                pointed_context,
                                "It is encouraged to fill the field {} on this node", field_name
                            );
                        }
                    }
                    NodeTypeFieldStatus::Deprecated => {
                        if node.field.0.contains_key(field_name) {
                            warn!(
                                pointed_context,
                                "Field {} is deprecated on this node type! Consider clearing it.",
                                field_name
                            );
                        }
                    }
                    NodeTypeFieldStatus::Optional => {
                        // Nothing to do.
                    }
                    }
                    if node
                        .field
                        .0
                        .get(field_name)
                        .is_some_and(|x| x.discriminant() != *field_type)
                    {
                        err!(pointed_context, "Field {} has the wrong type!", field_name);
                    }
                }

                // Extraneous fields
                for extra_field in
                    node.field.0.keys().filter(|body_field| {
                        !node_type.field.iter().map(|x| &x.0).contains(body_field)
                    })
                {
                    err!(
                        pointed_context,
                        "Extraneous field {} not allowed!",
                        extra_field
                    );
                }
            }
        }

        // Check edges ends so that they allow for that edge
        for last in 0..lc.total_event_count() {
            let ctx = EventIndex::from(last).before_and_including();
            let type_table = lc.types(ctx).collect::<HashMap<_, _>>();

            for pointed_context in ctx
                .index_iter()
                .filter(|e| !lc.is_stale(*e) && lc[e.thing()].is_edge())
            {
                // from end -> "outgoing" relative to that node
                // to end -> "incoming" relative to that node
                let edge = lc.edge(pointed_context);
                let outgoing_node = lc.node(edge.from);
                let incoming_node = lc.node(edge.to);

                let mut end_types_can_resolve = true;
                if !type_table.contains_key(outgoing_node.ty_name.as_str()) {
                    err!(
                        pointed_context,
                        "Edge head is at a Node with non-existent type {}",
                        outgoing_node.ty_name.as_str()
                    );
                    assert!(outgoing_node.ty.is_none());
                    end_types_can_resolve = false;
                }
                // COPY_PASTE
                if !type_table.contains_key(incoming_node.ty_name.as_str()) {
                    err!(
                        pointed_context,
                        "Edge tail is at a Node with non-existent type {}",
                        incoming_node.ty_name.as_str()
                    );
                    assert!(incoming_node.ty.is_none());
                    end_types_can_resolve = false;
                }

                if !end_types_can_resolve {
                    continue;
                }

                if !outgoing_node
                    .ty
                    .unwrap()
                    .outgoing
                    .iter()
                    .map(String::as_str)
                    .contains(edge.ty_name.as_str())
                {
                    err!(
                        pointed_context,
                        "Edge head node of type {} does not allow outgoing edge of type {}",
                        outgoing_node.ty_name.as_str(),
                        edge.ty_name.as_str()
                    );
                }
                // COPY_PASTE
                if !incoming_node
                    .ty
                    .unwrap()
                    .outgoing
                    .iter()
                    .map(String::as_str)
                    .contains(edge.ty_name.as_str())
                {
                    err!(
                        pointed_context,
                        "Edge head node of type {} does not allow outgoing edge of type {}",
                        incoming_node.ty_name.as_str(),
                        edge.ty_name.as_str()
                    );
                }
            }
        }

        // Check whether a child is allowed to override parent at the point of override
        for last in 0..lc.total_event_count() {
            let last = EventIndex::from(last);
            let ctx = last.before_and_including();
            let _type_table = lc.types(ctx);

            if lc[last].is_edge() || lc.raw_node(&last).parent.is_none() {
                continue;
            }

            let _current_node = lc.node(ctx.look_at(last));
            let _parent_node = lc.node(last.before().look_at(lc.raw_node(&last).parent.unwrap()));
        }
        r
    }
}
