//! The types here used to be a tightly integrated part of `lifecore-typecheck`
//! However, it is much more convenient to have types as a first-class citizen
//! But this creates issues with regard to how the type system should evolve in
//! the future. In general, if this module changes, databases with an older
//! instance of this type need careful modification to ensure typechecking
//! proceeds as normal

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use strum::Display;

use crate::{Body, LifeCoreError, Payload, TypedBody, ValueType};

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
                    (ValueType::Text, NodeTypeFieldStatus::Mandatory),
                ),
                (
                    "class".to_string(),
                    (ValueType::Serialized, NodeTypeFieldStatus::Mandatory),
                ),
                (
                    "from_end".to_string(),
                    (ValueType::Serialized, NodeTypeFieldStatus::Mandatory),
                ),
                (
                    "to_end".to_string(),
                    (ValueType::Serialized, NodeTypeFieldStatus::Mandatory),
                ),
                (
                    "additional_override".to_string(),
                    (ValueType::Serialized, NodeTypeFieldStatus::Mandatory),
                ),
                (
                    "field".to_string(),
                    (ValueType::Serialized, NodeTypeFieldStatus::Mandatory),
                ),
            ],
        }
    }

    fn name() -> &'static str {
        "Node_Type"
    }
}

impl From<NodeType> for Body {
    fn from(val: NodeType) -> Self {
        Body(
            vec![
                ("name", Payload::Text(val.name)),
                ("class", Payload::serialized_from(val.class)),
                ("from_end", Payload::serialized_from(val.outgoing)),
                ("to_end", Payload::serialized_from(val.incoming)),
                (
                    "additional_override",
                    Payload::serialized_from(val.additional_override),
                ),
                ("field", Payload::serialized_from(val.field)),
            ]
            .into_iter()
            .map(|(x, y)| (x.to_string(), y))
            .collect::<BTreeMap<String, Payload>>(),
        )
    }
}

impl TryFrom<&Body> for NodeType {
    type Error = LifeCoreError;

    fn try_from(v: &Body) -> Result<Self, Self::Error> {
        let name = v.get_text("name")?;
        let class: NodeTypeClass = v.get_deserialized("class")?;
        let from_end: Vec<String> = v.get_deserialized("from_end")?;
        let to_end: Vec<String> = v.get_deserialized("to_end")?;
        let additional_override: Vec<String> = v.get_deserialized("additional_override")?;
        let field: Vec<(String, (ValueType, NodeTypeFieldStatus))> = v.get_deserialized("field")?;

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

impl NodeType {
    pub fn new(
        name: &str,
        fields: &[(&str, (ValueType, NodeTypeFieldStatus))],
        outgoing: &[&str],
        incoming: &[&str],
        additional_override: &[&str],
    ) -> Self {
        Self {
            name: name.to_string(),
            class: NodeTypeClass::Regular,
            outgoing: outgoing.iter().map(|x| x.to_string()).collect(),
            incoming: incoming.iter().map(|x| x.to_string()).collect(),
            additional_override: additional_override.iter().map(|x| x.to_string()).collect(),
            field: fields
                .iter()
                .map(|(name, value)| (name.to_string(), value.clone()))
                .collect(),
        }
    }
}

impl ValueType {
    /// Parser for a DSL for expressing types
    pub fn parse(s: &str) -> Self {}
}
