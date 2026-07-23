use std::str::FromStr;

use itertools::Itertools;
use lifecore::{Body, LifeCoreError, Payload};

use crate::{
    FieldDecl,
    FieldStatus::{self, Mandatory},
    NodeField, NodeType, Type, TypeCheckMessage, TypeCheckOutput, ValueType,
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
        use ValueType::*;
        let mut stack = Vec::<ValueType>::new();
        let err = Err(LifeCoreError::NodeFieldDeserializationError {
            target_type: Some("ValueType".to_string()),
            field_name: None,
        });

        if !value.is_sequence() {
            return err;
        }

        for op in value.clone().try_as_sequence().unwrap().iter() {
            match op {
            Payload::Text(primitive) => match primitive.as_str() {
            "PreciseTime" => stack.push(PreciseTime),
            "Text" => stack.push(Text),
            "Number" => stack.push(Number),
            _ => return err,
            },
            Payload::Sequence(payloads) => {
                if (payloads.len() != 2 && !payloads[0].is_text() && !payloads[1].is_number())
                    || (stack.len() as i64) < payloads[1].clone().try_as_number().unwrap()
                {
                    return err;
                }
                match (
                    payloads[0].clone().try_as_text().unwrap().as_str(),
                    payloads[1].clone().try_as_number().unwrap(),
                ) {
                ("Sequence", 1) => {
                    let arg = stack.pop().unwrap();
                    stack.push(Sequence(Box::new(arg)));
                }
                ("Tuple", n) => {
                    let args = (0..n).map_while(|_| stack.pop()).collect_vec();
                    stack.push(Tuple(args));
                }
                ("OrderedSet", 1) => {
                    let arg = stack.pop().unwrap();
                    stack.push(OrderedSet(Box::new(arg)));
                }
                ("OneOf", n) => {
                    let args = (0..n).map_while(|_| stack.pop()).collect_vec();
                    stack.push(OneOf(args));
                }
                _ => return err,
                }
            }
            _ => return err,
            }
        }

        if stack.len() != 1 {
            return err;
        } else {
            Ok(stack[0].clone())
        }
    }
}

impl Into<Payload> for ValueType {
    fn into(self) -> Payload {
        use Payload::*;
        Sequence({
            match self {
            ValueType::PreciseTime => vec![Text("PreciseTime".to_string())],
            ValueType::Text => vec![Text("Text".to_string())],
            ValueType::Number => vec![Text("Number".to_string())],
            ValueType::Sequence(value_type) => vec![
                (*value_type).into(),
                Sequence(vec![Text("Sequence".to_string()), Number(1)]),
            ],
            ValueType::Tuple(value_types) => [
                value_types
                    .clone()
                    .into_iter()
                    .map(|x| x.into())
                    .collect_vec(),
                vec![Sequence(vec![
                    Text("Tuple".to_string()),
                    Number(value_types.len() as i64),
                ])],
            ]
            .concat(),
            ValueType::OrderedSet(value_type) => vec![
                (*value_type).into(),
                Sequence(vec![Text("OrderedSet".to_string()), Number(1)]),
            ],
            ValueType::OneOf(value_types) => [
                value_types
                    .clone()
                    .into_iter()
                    .map(|x| x.into())
                    .collect_vec(),
                vec![Sequence(vec![
                    Text("OneOf".to_string()),
                    Number(value_types.len() as i64),
                ])],
            ]
            .concat(),
            }
        })
    }
}

impl<'a> NodeField<'a> for ValueType {
    fn describe_self() -> ValueType {
        use ValueType::*;
        Sequence(Box::new(OneOf(vec![Text, Tuple(vec![Text, Number])])))
    }
}

impl<'a> TryFrom<&'a Payload> for FieldDecl {
    type Error = LifeCoreError;

    fn try_from(value: &'a Payload) -> Result<Self, Self::Error> {
        todo!()
    }
}

impl Into<Payload> for FieldDecl {
    fn into(self) -> Payload {
        todo!()
    }
}

impl<'a> NodeField<'a> for FieldDecl {
    fn describe_self() -> ValueType {
        use ValueType::*;
        OrderedSet(Box::new(Tuple(vec![
            Text,
            ValueType::describe_self(),
            FieldStatus::describe_self(),
        ])))
    }
}

impl<'a> TryFrom<&'a Body> for Type {
    type Error = LifeCoreError;

    fn try_from(value: &'a Body) -> Result<Self, Self::Error> {
        todo!()
    }
}

impl Into<Body> for Type {
    fn into(self) -> Body {
        todo!()
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
                    status: Mandatory,
                },
                FieldDecl {
                    name: "outgoing".to_string(),
                    ty: ValueType::OrderedSet(Box::new(ValueType::Text)),
                    status: Mandatory,
                },
                FieldDecl {
                    name: "incoming".to_string(),
                    ty: ValueType::OrderedSet(Box::new(ValueType::Text)),
                    status: Mandatory,
                },
                FieldDecl {
                    name: "additional_override".to_string(),
                    ty: ValueType::OrderedSet(Box::new(ValueType::Text)),
                    status: Mandatory,
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
