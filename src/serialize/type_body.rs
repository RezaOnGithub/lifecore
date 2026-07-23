use crate::{
    Body,
    typing::{FieldDecl, Type, ValueType},
};

use super::{Error, NodeField};

/// Converts an owned Rust value to and from one graph node body.
pub trait NodeBody: Sized {
    const TYPE_NAME: &'static str;

    fn describe_self() -> Type;
    fn into_body(self) -> Body;
    fn try_from_body(value: &Body) -> Result<Self, Error>;
}

impl NodeBody for Type {
    const TYPE_NAME: &'static str = "Type";

    fn describe_self() -> Type {
        Type::new(
            Self::TYPE_NAME,
            &[
                (
                    "name",
                    (ValueType::Text, crate::typing::FieldStatus::Mandatory),
                ),
                (
                    "body",
                    (
                        ValueType::OrderedSet(Box::new(FieldDecl::describe_self())),
                        crate::typing::FieldStatus::Optional,
                    ),
                ),
                (
                    "outgoing",
                    (
                        ValueType::OrderedSet(Box::new(ValueType::Text)),
                        crate::typing::FieldStatus::Optional,
                    ),
                ),
                (
                    "incoming",
                    (
                        ValueType::OrderedSet(Box::new(ValueType::Text)),
                        crate::typing::FieldStatus::Optional,
                    ),
                ),
                (
                    "additional_override",
                    (
                        ValueType::OrderedSet(Box::new(ValueType::Text)),
                        crate::typing::FieldStatus::Optional,
                    ),
                ),
            ],
            &[],
            &[],
            &[],
        )
    }

    fn into_body(self) -> Body {
        Body::from([
            ("name", self.name.into_payload()),
            ("body", self.body.into_payload()),
            ("outgoing", self.outgoing.into_payload()),
            ("incoming", self.incoming.into_payload()),
            (
                "additional_override",
                self.additional_override.into_payload(),
            ),
        ])
    }

    fn try_from_body(value: &Body) -> Result<Self, Error> {
        fn required<T: NodeField>(body: &Body, field_name: &str) -> Result<T, Error> {
            let value = body
                .get(field_name)
                .ok_or_else(|| Error::MissingBodyField {
                    field_name: field_name.to_string(),
                })?;
            T::try_from_payload(value).map_err(|source| Error::invalid_field(field_name, source))
        }

        fn optional_set<T>(body: &Body, field_name: &str) -> Result<crate::OrderedSet<T>, Error>
        where
            T: NodeField + Eq + std::hash::Hash,
        {
            match body.get(field_name) {
            Some(value) => crate::OrderedSet::try_from_payload(value)
                .map_err(|source| Error::invalid_field(field_name, source)),
            None => Ok(crate::OrderedSet::new()),
            }
        }

        Ok(Self {
            name: required(value, "name")?,
            body: optional_set(value, "body")?,
            outgoing: optional_set(value, "outgoing")?,
            incoming: optional_set(value, "incoming")?,
            additional_override: optional_set(value, "additional_override")?,
        })
    }
}
