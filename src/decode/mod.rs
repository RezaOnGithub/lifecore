// Typed decoding of node bodies and payload values, and canonical
// declarations derived from Rust types. SPEC 6.5 owns the rules.
//
// The derives live in `lifecore-derive` and are re-exported here beside the
// traits of the same names. Their generated code names only absolute paths
// (`::core`, `::std`, and this crate's public items through `::lifecore`), so
// it compiles wherever `lifecore` is a dependency under its own name. A crate
// that renames the dependency passes `#[lifecore(crate = "path")]` on the
// type; this crate declares `extern crate self as lifecore` so that its own
// tests derive the same way.

use crate::{
    Body, Date, EntryId, Node, OffsetTimestamp, Payload, PayloadKind, SignedDuration, Timestamp,
    typing::{FieldDecl, PathPart, Type, ValueType, kind_word, path_text},
};
use std::{collections::BTreeMap, fmt};

pub use lifecore_derive::{Declare, FromBody, FromPayload};

// A payload value of one shape. `shape` is what a declaration of a field of
// this type says, and a value `from_payload` accepts meets it. A recursive
// type has no finite shape, so its `shape` does not terminate.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be decoded from a payload value",
    note = "derive `FromBody` for a struct and `FromPayload` for an enum; `Option<T>` is accepted only as a struct field"
)]
pub trait FromPayload: Sized {
    fn shape() -> ValueType;
    // Errors carry their path relative to `value`; a caller decoding a part
    // of a larger value adds its own part in front (`DecodeError::under`).
    fn from_payload(value: &Payload) -> Result<Self, DecodeError>;
}

// A body of fields: a node's payload, a record's value, or a variant's.
// `fields` declares the fields `from_body` reads: `mandatory` for a field read
// as `T`, and `optional` (or the policy its attribute names) for `Option<T>`.
pub trait FromBody: Sized {
    fn fields() -> BTreeMap<String, FieldDecl>;
    fn from_body(body: &Body) -> Result<Self, DecodeError>;
}

// A type whose fields and attributes give a canonical declaration.
pub trait Declare {
    fn declaration() -> Type;
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, thiserror::Error)]
pub struct DecodeError {
    // From the decoded value to the part that failed; empty for the value
    // itself.
    pub path: Vec<PathPart>,
    pub problem: DecodeProblem,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum DecodeProblem {
    // A field read as `T`, not `Option<T>`, is absent.
    Missing,
    // An enum is expected as text, so `expected` is a kind, never a shape.
    WrongKind {
        expected: PayloadKind,
        found: PayloadKind,
    },
    // Text that names no variant of a unit-only enum.
    NotAMember {
        value: String,
    },
    // A variant whose tag names no variant of the enum; its body is not read.
    UnknownTag {
        tag: String,
    },
}

impl DecodeError {
    pub fn new(problem: DecodeProblem) -> Self {
        Self {
            path: vec![],
            problem,
        }
    }
    pub fn wrong_kind(expected: PayloadKind, value: &Payload) -> Self {
        Self::new(DecodeProblem::WrongKind {
            expected,
            found: value.kind(),
        })
    }
    // The same error, seen from the value that contains this one at `part`.
    pub fn under(mut self, part: PathPart) -> Self {
        self.path.insert(0, part);
        self
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            f.write_str("the value ")?;
        } else {
            write!(f, "field {} ", path_text(&self.path))?;
        }
        match &self.problem {
        DecodeProblem::Missing => f.write_str("is missing"),
        DecodeProblem::WrongKind { expected, found } => write!(
            f,
            "should be {}, found {}",
            kind_word(*expected),
            kind_word(*found)
        ),
        DecodeProblem::NotAMember { value } => {
            write!(f, "is {value:?}, which the decode type does not list")
        }
        DecodeProblem::UnknownTag { tag } => {
            write!(f, "has tag {tag:?}, which the decode type does not know")
        }
        }
    }
}

// A node whose body did not decode. The type name only serves the message:
// decoding never reads it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, thiserror::Error)]
#[error("cannot decode {ty_name:?} node {}: {error}", node.position())]
pub struct PayloadError {
    pub node: EntryId,
    pub ty_name: String,
    pub error: DecodeError,
}

impl Node {
    pub fn decode<T: FromBody>(&self) -> Result<T, PayloadError> {
        T::from_body(&self.payload).map_err(|error| PayloadError {
            node: self.id,
            ty_name: self.ty_name.clone(),
            error,
        })
    }
}

// A field read as `T`: absence is `Missing`, and errors are under its name.
pub fn field<T: FromPayload>(body: &Body, name: &str) -> Result<T, DecodeError> {
    optional_field(body, name)?
        .ok_or_else(|| DecodeError::new(DecodeProblem::Missing).under(PathPart::Field(name.into())))
}

// A field read as `Option<T>`: absent is `None`, and a present value must
// decode. There is no null, so absence is the only `None`.
pub fn optional_field<T: FromPayload>(body: &Body, name: &str) -> Result<Option<T>, DecodeError> {
    body.get(name)
        .map(|value| T::from_payload(value).map_err(|e| e.under(PathPart::Field(name.into()))))
        .transpose()
}

pub fn text(value: &Payload) -> Result<&str, DecodeError> {
    match value {
    Payload::Text(text) => Ok(text),
    other => Err(DecodeError::wrong_kind(PayloadKind::Text, other)),
    }
}

pub fn record(value: &Payload) -> Result<&Body, DecodeError> {
    match value {
    Payload::Record(body) => Ok(body),
    other => Err(DecodeError::wrong_kind(PayloadKind::Record, other)),
    }
}

// A variant's tag and body.
pub fn variant(value: &Payload) -> Result<(&str, &Body), DecodeError> {
    match value {
    Payload::Variant { tag, value } => Ok((tag, value)),
    other => Err(DecodeError::wrong_kind(PayloadKind::Variant, other)),
    }
}

macro_rules! scalar {
    ($($rust:ty => $variant:ident),* $(,)?) => {$(
        impl FromPayload for $rust {
            fn shape() -> ValueType {
                ValueType::$variant
            }
            fn from_payload(value: &Payload) -> Result<Self, DecodeError> {
                match value {
                Payload::$variant(value) => Ok(Clone::clone(value)),
                other => Err(DecodeError::wrong_kind(PayloadKind::$variant, other)),
                }
            }
        }
    )*};
}

scalar! {
    String => Text,
    i64 => Integer,
    bool => Boolean,
    Timestamp => Timestamp,
    SignedDuration => Duration,
    Date => Date,
    OffsetTimestamp => OffsetTimestamp,
}

impl<T: FromPayload> FromPayload for Vec<T> {
    fn shape() -> ValueType {
        ValueType::List {
            element: Box::new(T::shape()),
        }
    }
    fn from_payload(value: &Payload) -> Result<Self, DecodeError> {
        let Payload::List(values) = value else {
            return Err(DecodeError::wrong_kind(PayloadKind::List, value));
        };
        values
            .iter()
            .enumerate()
            .map(|(i, value)| T::from_payload(value).map_err(|e| e.under(PathPart::Index(i))))
            .collect()
    }
}

#[cfg(test)]
mod test;
