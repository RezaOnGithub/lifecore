use crate::{FieldDecl, FieldStatus, NodeField, NodeType, Type, ValueType};
use lifecore::{Body, LifeCoreError, OrderedSet, Payload};

/// *Note:* [`FieldStatus`] probably shouldn't be a field all by itself;
/// Currently the intent is to serialize it as a part of [`FieldDecl`]
mod field_status_as_a_field {
    use super::*;

    impl<'a> TryFrom<&'a Payload> for FieldStatus {
        type Error = LifeCoreError;

        fn try_from(value: &'a Payload) -> Result<Self, Self::Error> {
            let Payload::Text(value) = value else {
                return Err(LifeCoreError::Bap);
            };
            value.parse()
        }
    }

    impl From<FieldStatus> for Payload {
        fn from(value: FieldStatus) -> Self {
            Payload::Text(value.to_string())
        }
    }

    impl<'a> NodeField<'a> for FieldStatus {
        fn describe_self() -> ValueType {
            ValueType::OneOf(vec![
                ValueType::TextLiteral("Mandatory".to_string()),
                ValueType::TextLiteral("mandatory".to_string()),
                ValueType::TextLiteral("Encouraged".to_string()),
                ValueType::TextLiteral("encouraged".to_string()),
                ValueType::TextLiteral("Deprecated".to_string()),
                ValueType::TextLiteral("deprecated".to_string()),
                ValueType::TextLiteral("Optional".to_string()),
                ValueType::TextLiteral("optional".to_string()),
            ])
        }
    }
}

/// *Note:* [`ValueType`] probably shouldn't be a field all by itself;
/// Currently the intent is to serialize it as a part of [`FieldDecl`]
mod value_type_as_a_field {
    use super::*;
    impl<'a> TryFrom<&'a Payload> for ValueType {
        type Error = LifeCoreError;

        fn try_from(value: &'a Payload) -> Result<Self, Self::Error> {
            let err = Err(LifeCoreError::NodeFieldDeserializationError {
                target_type: Some("ValueType".to_string()),
                field_name: None,
            });
            let Payload::Text(s) = value else {
                return err;
            };
            s.parse()
        }
    }

    impl From<ValueType> for Payload {
        fn from(value: ValueType) -> Self {
            Payload::Text(value.to_string())
        }
    }

    impl<'a> NodeField<'a> for ValueType {
        fn describe_self() -> ValueType {
            ValueType::Text
        }
    }
}

mod field_decl_as_a_field {
    use super::*;

    impl<'a> TryFrom<&'a Payload> for FieldDecl {
        type Error = LifeCoreError;

        fn try_from(value: &'a Payload) -> Result<Self, Self::Error> {
            fn error() -> LifeCoreError {
                LifeCoreError::Bap
            }

            let Payload::Sequence(values) = value else {
                return Err(error());
            };
            let [Payload::Text(name), ty, status] = values.as_slice() else {
                return Err(error());
            };

            Ok(Self {
                name: name.clone(),
                ty: ValueType::try_from(ty).map_err(|_| error())?,
                status: FieldStatus::try_from(status).map_err(|_| error())?,
            })
        }
    }

    impl From<FieldDecl> for Payload {
        fn from(value: FieldDecl) -> Self {
            Payload::Sequence(vec![
                Payload::Text(value.name),
                value.ty.into(),
                value.status.into(),
            ])
        }
    }

    impl<'a> NodeField<'a> for FieldDecl {
        fn describe_self() -> ValueType {
            ValueType::Tuple(vec![
                ValueType::Text,
                ValueType::describe_self(),
                FieldStatus::describe_self(),
            ])
        }
    }
}

mod type_as_node_body {
    use super::*;
    impl<'a> TryFrom<&'a Body> for Type {
        type Error = LifeCoreError;

        fn try_from(value: &'a Body) -> Result<Self, Self::Error> {
            fn ordered_set<T>(
                body: &Body,
                field_name: &str,
                convert: impl Fn(&Payload) -> Result<T, LifeCoreError>,
            ) -> Result<OrderedSet<T>, LifeCoreError>
            where
                T: Eq + std::hash::Hash,
            {
                let Some(value) = body.0.get(field_name) else {
                    return Ok(OrderedSet::default());
                };
                let Payload::OrderedSet(values) = value else {
                    return Err(LifeCoreError::FieldContainsUnexpectedType {
                        field_name: field_name.to_string(),
                    });
                };

                values
                    .iter()
                    .map(convert)
                    .collect::<Result<_, _>>()
                    .map_err(|_| LifeCoreError::FieldDeserializationFailed {
                        field_name: field_name.to_string(),
                    })
            }

            fn text(value: &Payload) -> Result<String, LifeCoreError> {
                let Payload::Text(value) = value else {
                    return Err(LifeCoreError::Bap);
                };
                Ok(value.clone())
            }

            let name = match value.0.get("name") {
            Some(Payload::Text(name)) => name.clone(),
            Some(_) => {
                return Err(LifeCoreError::FieldContainsUnexpectedType {
                    field_name: "name".to_string(),
                });
            }
            None => {
                return Err(LifeCoreError::FieldNotFound {
                    field_name: "name".to_string(),
                });
            }
            };

            Ok(Self {
                name,
                body: ordered_set(value, "body", |value| FieldDecl::try_from(value))?,
                outgoing: ordered_set(value, "outgoing", text)?,
                incoming: ordered_set(value, "incoming", text)?,
                additional_override: ordered_set(value, "additional_override", text)?,
            })
        }
    }

    impl From<Type> for Body {
        fn from(value: Type) -> Self {
            let body = value.body.iter().cloned().map(Into::into).collect();
            let outgoing = value.outgoing.iter().cloned().map(Payload::Text).collect();
            let incoming = value.incoming.iter().cloned().map(Payload::Text).collect();
            let additional_override = value
                .additional_override
                .iter()
                .cloned()
                .map(Payload::Text)
                .collect();

            Body(
                [
                    ("name".to_string(), Payload::Text(value.name)),
                    ("body".to_string(), Payload::OrderedSet(body)),
                    ("outgoing".to_string(), Payload::OrderedSet(outgoing)),
                    ("incoming".to_string(), Payload::OrderedSet(incoming)),
                    (
                        "additional_override".to_string(),
                        Payload::OrderedSet(additional_override),
                    ),
                ]
                .into_iter()
                .collect(),
            )
        }
    }

    impl<'a> NodeType<'a> for Type {
        fn describe_self() -> Type {
            Type {
                name: "Type".to_string(),
                body: [
                    FieldDecl {
                        name: "name".to_string(),
                        ty: ValueType::Text,
                        status: FieldStatus::Mandatory,
                    },
                    FieldDecl {
                        name: "body".to_string(),
                        ty: ValueType::OrderedSet(Box::new(FieldDecl::describe_self())),
                        status: FieldStatus::Optional,
                    },
                    FieldDecl {
                        name: "outgoing".to_string(),
                        ty: ValueType::OrderedSet(Box::new(ValueType::Text)),
                        status: FieldStatus::Optional,
                    },
                    FieldDecl {
                        name: "incoming".to_string(),
                        ty: ValueType::OrderedSet(Box::new(ValueType::Text)),
                        status: FieldStatus::Optional,
                    },
                    FieldDecl {
                        name: "additional_override".to_string(),
                        ty: ValueType::OrderedSet(Box::new(ValueType::Text)),
                        status: FieldStatus::Optional,
                    },
                ]
                .into_iter()
                .collect(),
                outgoing: Default::default(),
                incoming: Default::default(),
                additional_override: Default::default(),
            }
        }

        fn name() -> &'static str {
            "Type"
        }
    }
}
