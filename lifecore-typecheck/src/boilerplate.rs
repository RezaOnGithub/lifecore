use lifecore::{Body, LifeCore, LifeCoreError, OrderedSet, Payload};

use crate::{
    FieldDecl,
    FieldStatus::{self, Mandatory},
    NodeField, NodeType, Type, TypeCheckFactory, TypeCheckMessage, TypeCheckOutput, ValueType,
};

impl Type {
    pub fn new(
        name: &str,
        fields: &[(&str, (ValueType, FieldStatus))],
        outgoing: &[&str],
        incoming: &[&str],
        additional_override: &[&str],
    ) -> Self {
        Self {
            name: name.to_string(),
            outgoing: outgoing.iter().map(|x| x.to_string()).collect(),
            incoming: incoming.iter().map(|x| x.to_string()).collect(),
            additional_override: additional_override.iter().map(|x| x.to_string()).collect(),
            body: fields
                .iter()
                .map(|(name, value)| FieldDecl {
                    name: name.to_string(),
                    ty: value.0.clone(),
                    status: value.1,
                })
                .collect(),
        }
    }
}

impl<'a> TryFrom<&'a Payload> for FieldStatus {
    type Error = LifeCoreError;

    fn try_from(value: &'a Payload) -> Result<Self, Self::Error> {
        let err = Err(LifeCoreError::Bap);
        if !value.is_text() {
            return err;
        }
        match value.clone().try_as_text().unwrap().as_str() {
        "Mandatory" => Ok(Self::Mandatory),
        "Encouraged" => Ok(Self::Encouraged),
        "Deprecated" => Ok(Self::Deprecated),
        "Optional" => Ok(Self::Optional),
        _ => err,
        }
    }
}

impl Into<Payload> for FieldStatus {
    fn into(self) -> Payload {
        match self {
        FieldStatus::Mandatory => Payload::text("Mandatory"),
        FieldStatus::Encouraged => Payload::text("Encouraged"),
        FieldStatus::Deprecated => Payload::text("Deprecated"),
        FieldStatus::Optional => Payload::text("Optional"),
        }
    }
}

impl<'a> NodeField<'a> for FieldStatus {
    fn describe_self() -> ValueType {
        ValueType::Text
    }
}

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

impl Into<Payload> for ValueType {
    fn into(self) -> Payload {
        Payload::Text(self.to_string())
    }
}

impl<'a> NodeField<'a> for ValueType {
    fn describe_self() -> ValueType {
        ValueType::Text
    }
}

impl<'a> TryFrom<&'a Payload> for FieldDecl {
    type Error = LifeCoreError;

    fn try_from(value: &'a Payload) -> Result<Self, Self::Error> {
        let Payload::Text(value) = value else {
            return Err(LifeCoreError::NodeFieldDeserializationError {
                target_type: Some("FieldDecl".to_string()),
                field_name: None,
            });
        };
        value.parse()
    }
}

impl Into<Payload> for FieldDecl {
    fn into(self) -> Payload {
        Payload::Text(self.to_string())
    }
}

impl<'a> NodeField<'a> for FieldDecl {
    fn describe_self() -> ValueType {
        ValueType::Text
    }
}

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

impl Into<Body> for Type {
    fn into(self) -> Body {
        let body = self.body.iter().cloned().map(Into::into).collect();
        let outgoing = self.outgoing.iter().cloned().map(Payload::Text).collect();
        let incoming = self.incoming.iter().cloned().map(Payload::Text).collect();
        let additional_override = self
            .additional_override
            .iter()
            .cloned()
            .map(Payload::Text)
            .collect();

        Body(
            [
                ("name".to_string(), Payload::Text(self.name)),
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
                    status: Mandatory,
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

impl TypeCheckOutput {
    pub fn push(&mut self, m: TypeCheckMessage) {
        self.0.push(m);
    }
}

impl<LC: LifeCore> Default for TypeCheckFactory<LC> {
    fn default() -> Self {
        Self {
            _x: Default::default(),
        }
    }
}
