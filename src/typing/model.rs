use crate::OrderedSet;

/// Describes whether a declared field is required or merely advised.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum FieldStatus {
    Mandatory,
    Encouraged,
    Deprecated,
    Optional,
}

/// Describes the shape of a field payload.
#[derive(Clone, PartialEq, Eq, Hash, strum::EnumIs, strum::EnumTryAs, Debug)]
pub enum ValueType {
    PreciseTime,
    Text,
    Number,
    TextLiteral(String),
    NumberLiteral(i64),
    Sequence(Box<ValueType>),
    Tuple(Vec<ValueType>),
    OrderedSet(Box<ValueType>),
    OneOf(Vec<ValueType>),
}

/// One named field within a stored [`Type`].
#[derive(Clone, PartialEq, Eq, Hash)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub struct FieldDecl {
    name: String,
    ty: ValueType,
    status: FieldStatus,
}

impl FieldDecl {
    pub fn new(name: impl Into<String>, ty: ValueType, status: FieldStatus) -> Self {
        Self {
            name: name.into(),
            ty,
            status,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn value_type(&self) -> &ValueType {
        &self.ty
    }

    pub fn status(&self) -> FieldStatus {
        self.status
    }
}

/// A type declaration stored as a graph node body.
#[derive(Clone, PartialEq, Eq, Hash)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub struct Type {
    pub name: String,
    pub body: OrderedSet<FieldDecl>,
    pub outgoing: OrderedSet<String>,
    pub incoming: OrderedSet<String>,
    pub additional_override: OrderedSet<String>,
}
