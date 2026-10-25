//! Typed reads of the fields planning's rules read. TypeCheck rejects a node
//! whose payload does not fit its declaration, and section 3.4 keeps those
//! fields in every declaration, so the analyzer assumes they fit (PLANNING
//! §8, decision 53): a missing mandatory field or a value of another kind is
//! an error, which the runner reports as an analyzer failure.

use lifecore::{EntryId, Node, Payload, PayloadKind, Timestamp, typing::kind_word};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("field `{field}` of {ty} {node:?} {problem}")]
pub struct PayloadError {
    pub node: EntryId,
    pub ty: String,
    pub field: &'static str,
    pub problem: Problem,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    Missing,
    WrongKind {
        expected: PayloadKind,
        found: PayloadKind,
    },
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
        Self::Missing => f.write_str("is missing"),
        Self::WrongKind { expected, found } => write!(
            f,
            "should be {}, found {}",
            kind_word(*expected),
            kind_word(*found)
        ),
        }
    }
}

fn error(node: &Node, field: &'static str, problem: Problem) -> PayloadError {
    PayloadError {
        node: node.id,
        ty: node.ty_name.clone(),
        field,
        problem,
    }
}

fn read<'a, T>(
    node: &'a Node,
    field: &'static str,
    expected: PayloadKind,
    extract: impl Fn(&'a Payload) -> Option<T>,
) -> Result<Option<T>, PayloadError> {
    let Some(value) = node.payload.get(field) else {
        return Ok(None);
    };
    extract(value).map(Some).ok_or_else(|| {
        error(
            node,
            field,
            Problem::WrongKind {
                expected,
                found: value.kind(),
            },
        )
    })
}

fn mandatory<T>(
    node: &Node,
    field: &'static str,
    value: Result<Option<T>, PayloadError>,
) -> Result<T, PayloadError> {
    value?.ok_or_else(|| error(node, field, Problem::Missing))
}

pub fn optional_integer(node: &Node, field: &'static str) -> Result<Option<i64>, PayloadError> {
    read(node, field, PayloadKind::Integer, |value| match value {
    Payload::Integer(value) => Some(*value),
    _ => None,
    })
}

pub fn integer(node: &Node, field: &'static str) -> Result<i64, PayloadError> {
    mandatory(node, field, optional_integer(node, field))
}

pub fn optional_timestamp(
    node: &Node,
    field: &'static str,
) -> Result<Option<Timestamp>, PayloadError> {
    read(node, field, PayloadKind::Timestamp, |value| match value {
    Payload::Timestamp(value) => Some(*value),
    _ => None,
    })
}

pub fn timestamp(node: &Node, field: &'static str) -> Result<Timestamp, PayloadError> {
    mandatory(node, field, optional_timestamp(node, field))
}

pub fn text<'a>(node: &'a Node, field: &'static str) -> Result<&'a str, PayloadError> {
    mandatory(
        node,
        field,
        read(node, field, PayloadKind::Text, |value| match value {
        Payload::Text(value) => Some(value.as_str()),
        _ => None,
        }),
    )
}
