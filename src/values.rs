use crate::Timestamp;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type Body = BTreeMap<String, Payload>;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Payload {
    Text(String),
    Number(i64),
    Boolean(bool),
    PreciseTime(Timestamp),
    List(Vec<Payload>),
    Record(Body),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PayloadKind {
    Text,
    Number,
    Boolean,
    PreciseTime,
    List,
    Record,
}

impl Payload {
    pub fn kind(&self) -> PayloadKind {
        match self {
        Self::Text(_) => PayloadKind::Text,
        Self::Number(_) => PayloadKind::Number,
        Self::Boolean(_) => PayloadKind::Boolean,
        Self::PreciseTime(_) => PayloadKind::PreciseTime,
        Self::List(_) => PayloadKind::List,
        Self::Record(_) => PayloadKind::Record,
        }
    }
}

impl From<&str> for Payload {
    fn from(v: &str) -> Self {
        Self::Text(v.into())
    }
}
impl From<String> for Payload {
    fn from(v: String) -> Self {
        Self::Text(v)
    }
}
impl From<i64> for Payload {
    fn from(v: i64) -> Self {
        Self::Number(v)
    }
}
impl From<bool> for Payload {
    fn from(v: bool) -> Self {
        Self::Boolean(v)
    }
}
impl From<Timestamp> for Payload {
    fn from(v: Timestamp) -> Self {
        Self::PreciseTime(v)
    }
}
impl From<Vec<Payload>> for Payload {
    fn from(v: Vec<Payload>) -> Self {
        Self::List(v)
    }
}
impl From<Body> for Payload {
    fn from(v: Body) -> Self {
        Self::Record(v)
    }
}
