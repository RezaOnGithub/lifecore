use std::hash::Hash;

use crate::{
    OrderedSet, Payload, Timestamp,
    typing::{FieldDecl, FieldStatus, ValueType},
};

use super::Error;

/// Converts one owned Rust value to and from one graph payload.
pub trait NodeField: Sized {
    fn describe_self() -> ValueType;
    fn into_payload(self) -> Payload;
    fn try_from_payload(value: &Payload) -> Result<Self, Error>;
}

impl NodeField for String {
    fn describe_self() -> ValueType {
        ValueType::Text
    }

    fn into_payload(self) -> Payload {
        Payload::Text(self)
    }

    fn try_from_payload(value: &Payload) -> Result<Self, Error> {
        match value {
        Payload::Text(value) => Ok(value.clone()),
        _ => Err(Error::unexpected_payload(Self::describe_self(), value)),
        }
    }
}

impl NodeField for i64 {
    fn describe_self() -> ValueType {
        ValueType::Number
    }

    fn into_payload(self) -> Payload {
        Payload::Number(self)
    }

    fn try_from_payload(value: &Payload) -> Result<Self, Error> {
        match value {
        Payload::Number(value) => Ok(*value),
        _ => Err(Error::unexpected_payload(Self::describe_self(), value)),
        }
    }
}

impl NodeField for Timestamp {
    fn describe_self() -> ValueType {
        ValueType::PreciseTime
    }

    fn into_payload(self) -> Payload {
        Payload::PreciseTime(self)
    }

    fn try_from_payload(value: &Payload) -> Result<Self, Error> {
        match value {
        Payload::PreciseTime(value) => Ok(value.clone()),
        _ => Err(Error::unexpected_payload(Self::describe_self(), value)),
        }
    }
}

impl NodeField for FieldStatus {
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

    fn into_payload(self) -> Payload {
        Payload::Text(self.to_string())
    }

    fn try_from_payload(value: &Payload) -> Result<Self, Error> {
        let Payload::Text(value) = value else {
            return Err(Error::unexpected_payload(Self::describe_self(), value));
        };
        value.parse().map_err(|_| Error::InvalidFieldDeclSyntax)
    }
}

impl NodeField for ValueType {
    fn describe_self() -> ValueType {
        ValueType::Text
    }

    fn into_payload(self) -> Payload {
        Payload::Text(self.to_string())
    }

    fn try_from_payload(value: &Payload) -> Result<Self, Error> {
        let Payload::Text(value) = value else {
            return Err(Error::unexpected_payload(Self::describe_self(), value));
        };
        value.parse()
    }
}

impl NodeField for FieldDecl {
    fn describe_self() -> ValueType {
        ValueType::Tuple(vec![
            ValueType::Text,
            ValueType::describe_self(),
            FieldStatus::describe_self(),
        ])
    }

    fn into_payload(self) -> Payload {
        Payload::Sequence(vec![
            Payload::Text(self.name().to_string()),
            self.value_type().clone().into_payload(),
            self.status().into_payload(),
        ])
    }

    fn try_from_payload(value: &Payload) -> Result<Self, Error> {
        let Payload::Sequence(values) = value else {
            return Err(Error::unexpected_payload(Self::describe_self(), value));
        };
        let [name, value_type, status] = values.as_slice() else {
            return Err(Error::InvalidTupleArity {
                expected: 3,
                actual: values.len(),
            });
        };
        let name =
            String::try_from_payload(name).map_err(|source| Error::invalid_element(0, source))?;
        let value_type = ValueType::try_from_payload(value_type)
            .map_err(|source| Error::invalid_element(1, source))?;
        let status = FieldStatus::try_from_payload(status)
            .map_err(|source| Error::invalid_element(2, source))?;
        Ok(Self::new(name, value_type, status))
    }
}

impl<T> NodeField for OrderedSet<T>
where
    T: NodeField + Eq + Hash,
{
    fn describe_self() -> ValueType {
        ValueType::OrderedSet(Box::new(T::describe_self()))
    }

    fn into_payload(self) -> Payload {
        Payload::OrderedSet(self.into_iter().map(NodeField::into_payload).collect())
    }

    fn try_from_payload(value: &Payload) -> Result<Self, Error> {
        let Payload::OrderedSet(values) = value else {
            return Err(Error::unexpected_payload(Self::describe_self(), value));
        };
        values
            .iter()
            .enumerate()
            .map(|(index, value)| {
                T::try_from_payload(value).map_err(|source| Error::invalid_element(index, source))
            })
            .collect()
    }
}
