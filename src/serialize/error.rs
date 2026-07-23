use strum::IntoDiscriminant;

use crate::{Payload, PayloadKind, typing::ValueType};

/// Failures while converting between typed values and graph storage values.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("unexpected payload kind; expected {expected}, found {actual}")]
    UnexpectedPayloadKind {
        expected: ValueType,
        actual: PayloadKind,
    },
    #[error("missing body field {field_name:?}")]
    MissingBodyField { field_name: String },
    #[error("invalid body field {field_name:?}")]
    InvalidBodyField {
        field_name: String,
        #[source]
        source: Box<Self>,
    },
    #[error("invalid collection element at index {index}")]
    InvalidCollectionElement {
        index: usize,
        #[source]
        source: Box<Self>,
    },
    #[error("invalid tuple arity; expected {expected}, found {actual}")]
    InvalidTupleArity { expected: usize, actual: usize },
    #[error("invalid value-type syntax")]
    InvalidValueTypeSyntax,
    #[error("invalid field-declaration syntax")]
    InvalidFieldDeclSyntax,
}

impl Error {
    pub(crate) fn unexpected_payload(expected: ValueType, actual: &Payload) -> Self {
        Self::UnexpectedPayloadKind {
            expected,
            actual: actual.discriminant(),
        }
    }

    pub(crate) fn invalid_field(field_name: impl Into<String>, source: Self) -> Self {
        Self::InvalidBodyField {
            field_name: field_name.into(),
            source: Box::new(source),
        }
    }

    pub(crate) fn invalid_element(index: usize, source: Self) -> Self {
        Self::InvalidCollectionElement {
            index,
            source: Box::new(source),
        }
    }
}
