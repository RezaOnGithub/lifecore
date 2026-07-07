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
//! This can be terribly inefficient, and of course there is a streaming approach but correctness for now. PERF
//!
//! Some Design Considerations
//! ==========================
//!
//! Imagine a scenario where a type with an associated constraint system has to be updated. Now constraint system can only:
//!         - Make a previously mandatory field optional
//!         - Add a new optional field
//! And outside of these two contrived scenarios, the constraint system now has to be forked and an entirely new types
//! should be used. With a different name also! Because names are important.
//!
//! One way of achieving a bit of sanity could be appending the "system version" to the node type, but it does not
//! address the main point of fragility of updating constraint systems: old validation should still work.
//!
//! If we had reusable system pieces and could encode the constraint in the graph itself using maybe script bytecode,
//! this could be addressed, and some people were crazy enough to do something like this on the neo-FreeNet if I
//! remember correctly.
//!
//! So in that hypothetical scenario, the constraint system could be encoded just like the types here and validated with
//! a similar algorithm: applying to all events before the current version of itself and after the previous version
//!
//! Override Field
//! ==============
//!
//! An additional thing for types: they have to declare the node types that can override them, and this node type should
//! be a valid nodetype at the time of definiton. Haven't considered effects of this decision yet...
//!
//! One type that needs to be practically, immediately added is Tombstone type, so that Node_Type and any other node
//! can be overriden and effectively disabled.
//!
//! Derive Macro
//! ============
//!
//! There should be a derive macro that creates a NodeType based on its body struct.
//!     - Basic types (like Timestamp, i64, String) get serialized as their own
//!     - Option becomes Optional, additional attributes for Encouraged and Deprecated
//!     - the rest are Serialized if they impl Serialize
//! Otherwise, error.

use itertools::Itertools;
use lifecore::analysis::*;
use lifecore::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum NodeTypeClass {
    Spike,
    Hub,
    Toothpick,
    EdgeNode,
    Regular,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum NodeTypeFieldStatus {
    Mandatory,
    Encouraged,
    Deprecated,
    Optional,
}

pub trait TypedBody<'a>: Into<NodeBody> + TryFrom<&'a NodeBody> {
    fn describe_self() -> NodeType;
}

// TODO some system of deriving From/Into between a concrete body and NodeBody
// Somehow, it should be smart enough to use "native" types for some fields and serialize the rest
#[derive(Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct NodeType {
    // This should become PayloadExt::Text
    pub name: String,
    // These should be serialized
    pub class: NodeTypeClass,
    pub outgoing: Vec<String>,
    pub incoming: Vec<String>,
    pub additional_override: Vec<String>,
    pub field: Vec<(String, (PayloadType, NodeTypeFieldStatus))>,
}

impl<'a> TypedBody<'a> for NodeType {
    fn describe_self() -> NodeType {
        NodeType {
            name: "Node_Type".into(),
            class: NodeTypeClass::Regular,
            outgoing: vec![],
            incoming: vec![],
            additional_override: vec!["Tombstone".to_string()],
            field: vec![
                (
                    "name".to_string(),
                    (PayloadType::Text, NodeTypeFieldStatus::Mandatory),
                ),
                (
                    "class".to_string(),
                    (PayloadType::Serialized, NodeTypeFieldStatus::Mandatory),
                ),
                (
                    "from_end".to_string(),
                    (PayloadType::Serialized, NodeTypeFieldStatus::Mandatory),
                ),
                (
                    "to_end".to_string(),
                    (PayloadType::Serialized, NodeTypeFieldStatus::Mandatory),
                ),
                (
                    "additional_override".to_string(),
                    (PayloadType::Serialized, NodeTypeFieldStatus::Mandatory),
                ),
                (
                    "field".to_string(),
                    (PayloadType::Serialized, NodeTypeFieldStatus::Mandatory),
                ),
            ],
        }
    }
}

impl Into<NodeBody> for NodeType {
    fn into(self) -> NodeBody {
        NodeBody(
            vec![
                ("name", PayloadExt::Text(self.name)),
                ("class", PayloadExt::serialized_from(self.class)),
                ("from_end", PayloadExt::serialized_from(self.outgoing)),
                ("to_end", PayloadExt::serialized_from(self.incoming)),
                (
                    "additional_override",
                    PayloadExt::serialized_from(self.additional_override),
                ),
                ("field", PayloadExt::serialized_from(self.field)),
            ]
            .into_iter()
            .map(|(x, y)| (x.to_string(), y))
            .collect::<HashMap<String, PayloadExt>>(),
        )
    }
}

impl TryFrom<&NodeBody> for NodeType {
    type Error = LifeCoreError;

    fn try_from(v: &NodeBody) -> Result<Self, Self::Error> {
        let name = v.get_text("name")?;
        let class: NodeTypeClass = v.get_deserialized("class")?;
        let from_end: Vec<String> = v.get_deserialized("from_end")?;
        let to_end: Vec<String> = v.get_deserialized("to_end")?;
        let additional_override: Vec<String> = v.get_deserialized("additional_override")?;
        let field: Vec<(String, (PayloadType, NodeTypeFieldStatus))> =
            v.get_deserialized("field")?;

        Ok(Self {
            name,
            class,
            outgoing: from_end,
            incoming: to_end,
            additional_override,
            field,
        })
    }
}

const TYPECHECK_NAME: &'static str = "Type_Check";

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
    fn initialize(&mut self, lc: &LifeCore) -> AnalysisOutput {
        AnalysisOutput(vec![])
    }

    // TODO PERF
    // TODO needs to be modified to return a "relevance_check" field on CSMessage to decide the messages we should be seeing.
    // Errors are always relevant, of course, but warnings less so because we may have resolved them in a future window.
    // Relevance check should be a commandline toggle
    fn analyze(&self, lc: &LifeCore) -> AnalysisOutput {
        let mut r = AnalysisOutput(vec![]);
        for last in lc.last().before_and_including() {
            // For every self.event[0..X]
            //      - Get the effective types at that point
            //      - Get first event S where effective type is one we currently have
            //      - All fresh nodes in self.event[0..X] must typecheck against those types
            let window = EventIndex(last as u64).before_and_including();
            let type_table = lc
                .fresh_nodes_in_range_of_type("Node_Type", window.clone())
                .map(|(e, x)| {
                    let t = NodeType::try_from(&lc.resolve(&x.payload)).unwrap();
                    (t.name.clone(), (e, t))
                })
                .collect::<HashMap<String, (EventIndex, NodeType)>>();
            // Check that override types exist
            for (ty_event, ty) in type_table.values() {
                for override_ty in ty.additional_override.iter() {
                    let message_template = ConstraintSystemMessage {
                        offending_event: *ty_event,
                        window: window.clone(),
                        system_name: TYPECHECK_NAME.to_string(),
                        system_config: self.config.clone(),
                        message: String::new(),
                    };
                    if !type_table.contains_key(override_ty) {
                        r.0.push(AnalysisReport::Inconsistency(message_template.new_message(
                            format!(
                                "Type {} has an unknown override type {}",
                                ty.name.as_str(),
                                override_ty
                            ),
                        )))
                    }
                }
            }
            // Check nodes for their fields, Check edges for their connectivity
            for e in lc.fresh_events_in_range(window.clone()) {
                let message_template = ConstraintSystemMessage {
                    offending_event: e.0,
                    window: window.clone(),
                    system_name: TYPECHECK_NAME.to_string(),
                    system_config: self.config.clone(),
                    message: String::new(),
                };
                // Not lambdas because variadics
                macro_rules! warn {
                    ($($y:expr),+) => (
                        r.0.push(AnalysisReport::Warning(message_template.new_message(
                            format!($($y),+)
                    )));)
                }
                macro_rules! err {
                    ($($y:expr),+) => (
                        r.0.push(AnalysisReport::Inconsistency(message_template.new_message(
                            format!($($y),+)
                    )));)
                }
                match e.1 {
                Event::Node(node) => {
                    let body = lc.resolve(&node.payload);
                    let type_name = &lc[node.ty];
                    if !type_table.contains_key(type_name) {
                        err!(
                            "Corresponding Node_Type not found in window {}..{}",
                            window.start,
                            window.end
                        );
                        continue;
                    }
                    let resolved_type = &type_table[type_name].1;

                    // Fields that may be present and mandatory
                    for (field_name, (field_type, field_policy)) in resolved_type.field.iter() {
                        match field_policy {
                        NodeTypeFieldStatus::Mandatory => {
                            if !body.0.contains_key(field_name) {
                                err!("Node does not have the mandatory field {}", field_name);
                            }
                        }
                        NodeTypeFieldStatus::Encouraged => {
                            if !body.0.contains_key(field_name) {
                                warn!(
                                    "It is encouraged to fill the field {} on this node",
                                    field_name
                                );
                            }
                        }
                        NodeTypeFieldStatus::Deprecated => {
                            if body.0.contains_key(field_name) {
                                warn!(
                                    "Field {} is deprecated on this node type! Consider clearing it.",
                                    field_name
                                );
                            }
                        }
                        NodeTypeFieldStatus::Optional => {
                            // Nothing to do.
                        }
                        }
                        if body.0.contains_key(field_name)
                            && body.0[field_name].discriminant() != *field_type
                        {
                            err!("Field {} has the wrong type!", field_name);
                        }
                    }

                    // Extraneous fields
                    for extra_field in body.0.keys().filter(|body_field| {
                        !resolved_type
                            .field
                            .iter()
                            .map(|x| &x.0)
                            .contains(body_field)
                    }) {
                        err!("Extraneous field {} not allowed!", extra_field);
                    }

                    // Override Logic. SUS
                    // First, check if the override types even exist

                    if node.parent.is_some() {
                        let parent_type_name = &lc[&lc.node(node.parent.unwrap()).ty];
                        if !type_table.contains_key(parent_type_name) {
                            // We have bigger problems, and this should be reported elsewhere
                            continue;
                        }
                        let parent_type = &type_table[parent_type_name].1;
                        if !(parent_type.name == resolved_type.name
                            || parent_type
                                .additional_override
                                .contains(&resolved_type.name))
                        {
                            err!(
                                "Node of type {} overrides a node of type {} but the latter does not allow children of the former type",
                                resolved_type.name,
                                parent_type.name
                            );
                        }
                    }
                }
                Event::Edge(edge) => {
                    // from end -> "outgoing" relative to that node
                    // to end -> "incoming" relative to that node
                    let outgoing_type_name = &lc[lc.node(edge.from).ty];
                    let incoming_type_name = &lc[lc.node(edge.to).ty];
                    if !type_table.contains_key(outgoing_type_name) {
                        err!(
                            "Edge head is at a Node with non-existent type {}",
                            outgoing_type_name
                        );
                        continue;
                    }
                    if !type_table.contains_key(incoming_type_name) {
                        err!(
                            "Edge tail is at a Node with non-existent type {}",
                            incoming_type_name
                        );
                        continue;
                    }
                    let outgoing_type = &type_table[outgoing_type_name].1;
                    let incoming_type = &type_table[incoming_type_name].1;
                    let edge_type_name = &lc[edge.ty];
                    if !outgoing_type
                        .outgoing
                        .iter()
                        .map(String::as_str)
                        .contains(edge_type_name)
                    {
                        err!(
                            "Edge head of type {} does not allow outgoing edge of type {}",
                            outgoing_type_name,
                            edge_type_name
                        );
                    }
                    if !incoming_type
                        .incoming
                        .iter()
                        .map(String::as_str)
                        .contains(edge_type_name)
                    {
                        err!(
                            "Edge tail of type {} does not allow incoming edge of type {}",
                            incoming_type_name,
                            edge_type_name
                        );
                    }
                }
                }
            }
        }
        r
    }
}
