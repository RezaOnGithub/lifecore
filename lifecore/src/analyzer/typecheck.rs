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

use crate::{
    AnalysisReport, Event, EventIndex, LifeCore, LifeCoreError, NodeBody, PayloadExt, PayloadType,
    analyzer::Analyzer,
};
use itertools::Itertools;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::{HashMap, HashSet};

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

// TODO some system of deriving From/Into between a concrete body and NodeBody
// Somehow, it should be smart enough to use "native" types for some fields and serialize the rest
#[derive(Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct NodeType {
    // This should become PayloadExt::Text
    name: String,
    // These should be serialized
    class: NodeTypeClass,
    from_end: Vec<String>,
    to_end: Vec<String>,
    additional_override: Vec<String>,
    field: Vec<(String, (PayloadType, NodeTypeFieldStatus))>,
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
            from_end,
            to_end,
            additional_override,
            field,
        })
    }
}

// Due to the very strict and picky system I've come up with, the very first event MUST be a node that
// declares that there is gonna be nodes that specify types; that is, a node of type "Node_Type" that
// declares "Node_Type". This bootstrap process needs to happen, and a non-stale node of type "Node_Type"
// declaring "Node_Type" must always exist at any EventIndex
pub(crate) fn describe_self() -> NodeBody {
    let nt = NodeType {
        name: "Node_Type".into(),
        class: NodeTypeClass::Regular,
        from_end: vec![],
        to_end: vec![],
        additional_override: vec![],
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
                "field".to_string(),
                (PayloadType::Serialized, NodeTypeFieldStatus::Mandatory),
            ),
        ],
    };
    NodeBody(
        vec![
            ("name", PayloadExt::Text(nt.name)),
            ("class", PayloadExt::serialized_from(nt.class)),
            ("from_end", PayloadExt::serialized_from(nt.from_end)),
            ("to_end", PayloadExt::serialized_from(nt.to_end)),
            (
                "additional_override",
                PayloadExt::serialized_from(nt.additional_override),
            ),
            ("field", PayloadExt::serialized_from(nt.field)),
        ]
        .into_iter()
        .map(|(x, y)| (x.to_string(), y))
        .collect::<HashMap<String, PayloadExt>>(),
    )
}

pub struct TypeCheck;

impl Analyzer for TypeCheck {
    fn initialize(lc: &mut crate::LifeCore, config: &str) -> Result<Self, crate::LifeCoreError> {
        todo!()
    }

    // TODO PERF
    fn analyze(self, lc: &LifeCore) -> Vec<crate::AnalysisReport> {
        let mut r = Vec::<AnalysisReport>::new();
        for last in lc.last().before_and_including() {
            // For every self.event[0..X]
            //      - Get the effective types at that point
            //      - Get first event S where effective type is one we currently have
            //      - All fresh nodes in self.event[0..X] must typecheck against those types
            let window = EventIndex(last as u64).before_and_including();
            let types = lc
                .fresh_nodes_in_range_of_type("Node_Type", window.clone())
                .map(|(_, x)| {
                    let t = NodeType::try_from(&lc.resolve(&x.payload)).unwrap();
                    (t.name.clone(), t)
                })
                .collect::<HashMap<String, NodeType>>();
            for e in lc.fresh_events_in_range(window) {
                match e.1 {
                    Event::Node(node) => {
                        let body = lc.resolve(&node.payload);
                        let ty = &lc[node.ty];
                        if !types.contains_key(ty) {
                            // Unknown Type!
                            // TODO
                            // r.push( );
                            continue;
                        }
                        let resolved_type = &types[ty];

                        // Fields that may be present and mandatory
                        for field in resolved_type.field.iter() {
                            match field.1.1 {
                                NodeTypeFieldStatus::Mandatory => todo!(),
                                NodeTypeFieldStatus::Encouraged => todo!(),
                                NodeTypeFieldStatus::Deprecated => todo!(),
                                NodeTypeFieldStatus::Optional => todo!(),
                            }
                        }

                        // Extraneous fields
                        for extra_field in body.0.keys().filter(|x| {
                            !resolved_type
                                .field
                                .iter()
                                .map(|x| x.0.clone())
                                .collect::<HashSet<String>>()
                                .contains(x.as_str())
                        }) {
                            // We don't allow extra field!
                            // TODO
                        }
                    }
                    Event::Edge(edge) => {
                        // from end -> "outgoing" relative to that node
                        // to end -> "incoming" relative to that node
                    }
                }
            }
        }
        r
    }
}
