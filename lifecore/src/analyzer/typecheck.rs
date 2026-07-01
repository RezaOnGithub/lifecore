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
    AnalysisReport, LifeCore, LifeCoreError, NodeBody, PayloadExt, PayloadType, analyzer::Analyzer,
};
use itertools::Itertools;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
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

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct NodeType {
    // This is a field that is gonna get deserialized as PayloadExt::Text.
    // This is intended. No problem.
    name: String,
    // This is also a field that will be deserialized to Payload::Text
    // This was not intended! How to deserialized correctly?
    //      - try with the string,
    //      - if that fails, try with SerializedExt
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
        let class: NodeTypeClass = v.get_serialized("class")?;
        let from_end: Vec<String> = v.get_serialized("form_end")?;
        let to_end: Vec<String> = v.get_serialized("to_end")?;
        let additional_override: Vec<String> = v.get_serialized("additional_override")?;
        let field: Vec<(String, (PayloadType, NodeTypeFieldStatus))> = v.get_serialized("field")?;

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
            ("name", PayloadExt::create(nt.name)),
            ("class", PayloadExt::create(nt.class)),
            ("from_end", PayloadExt::create(nt.from_end)),
            ("to_end", PayloadExt::create(nt.to_end)),
            (
                "additional_override",
                PayloadExt::create(nt.additional_override),
            ),
            ("field", PayloadExt::create(nt.field)),
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

    fn analyze(self, lc: &LifeCore) -> Vec<crate::AnalysisReport> {
        let mut r = Vec::<AnalysisReport>::new();
        for upper in lc.last().before_and_including() {
            // For every self.event[0..X]
            //      - Get the effective types at that point
            //      - Get first event S where effective type is one we currently have
            //      - All fresh nodes in self.event[0..X] must typecheck against those types
            let types = lc
                .fresh_nodes_in_range_of_type("Node_Type", 0..(upper + 1))
                .map(|(_, x)| {
                    let t = NodeType::try_from(&lc.resolve(&x.payload)).unwrap();
                    (t.name.clone(), t)
                })
                .collect::<HashMap<String, NodeType>>();
        }
        r
    }
}
