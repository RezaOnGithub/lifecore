//! The types here used to be a tightly integrated part of `lifecore-typecheck`
//! However, it is much more convenient to have types as a first-class citizen
//! But this creates issues with regard to how the type system should evolve in the future.
//! In general, if this module changes, databases with an older instance of this type need
//! careful modification to ensure typechecking proceeds as normal

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{LifeCoreError, NodeBodyExt, PayloadExt, PayloadType, TypedBody};

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone)]
pub struct NodeType {
    pub name: String,
    pub class: NodeTypeClass,
    pub outgoing: Vec<String>,
    pub incoming: Vec<String>,
    pub additional_override: Vec<String>,
    pub field: Vec<(String, (PayloadType, NodeTypeFieldStatus))>,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone)]
pub enum NodeTypeClass {
    Spike,
    Hub,
    Toothpick,
    EdgeNode,
    Regular,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone)]
pub enum NodeTypeFieldStatus {
    Mandatory,
    Encouraged,
    Deprecated,
    Optional,
}

impl<'a> TypedBody<'a> for NodeType {
    fn describe_self() -> NodeType {
        NodeType {
            name: Self::name().to_string(),
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

    fn name() -> &'static str {
        "Node_Type"
    }
}

impl From<NodeType> for NodeBodyExt {
    fn from(val: NodeType) -> Self {
        NodeBodyExt(
            vec![
                ("name", PayloadExt::Text(val.name)),
                ("class", PayloadExt::serialized_from(val.class)),
                ("from_end", PayloadExt::serialized_from(val.outgoing)),
                ("to_end", PayloadExt::serialized_from(val.incoming)),
                (
                    "additional_override",
                    PayloadExt::serialized_from(val.additional_override),
                ),
                ("field", PayloadExt::serialized_from(val.field)),
            ]
            .into_iter()
            .map(|(x, y)| (x.to_string(), y))
            .collect::<HashMap<String, PayloadExt>>(),
        )
    }
}

impl TryFrom<&NodeBodyExt> for NodeType {
    type Error = LifeCoreError;

    fn try_from(v: &NodeBodyExt) -> Result<Self, Self::Error> {
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