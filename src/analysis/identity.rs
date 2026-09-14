use crate::{Body, EntryId};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

use super::MessageType;

//A Facts retain typed values and enum tags after normalization. Records are
//A ordered; lists preserve order. Analyzers normalize their own domain sets.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Fact {
    Null,
    Boolean(bool),
    Integer(i64),
    Unsigned(u64),
    Text(String),
    List(Vec<Fact>),
    Record(BTreeMap<String, Fact>),
}

impl Fact {
    pub fn from_typed(value: &impl Serialize) -> Result<Self, serde_json::Error> {
        fn convert(value: serde_json::Value) -> Result<Fact, serde_json::Error> {
            use serde_json::Value;
            Ok(match value {
            Value::Null => Fact::Null,
            Value::Bool(v) => Fact::Boolean(v),
            Value::String(v) => Fact::Text(v),
            Value::Number(v) => {
                if let Some(v) = v.as_i64() {
                    Fact::Integer(v)
                } else if let Some(v) = v.as_u64() {
                    Fact::Unsigned(v)
                } else {
                    return Err(<serde_json::Error as serde::de::Error>::custom(
                        "semantic facts require exact integers, not floating point",
                    ));
                }
            }
            Value::Array(v) => Fact::List(v.into_iter().map(convert).collect::<Result<_, _>>()?),
            Value::Object(v) => Fact::Record(
                v.into_iter()
                    .map(|(key, value)| Ok((key, convert(value)?)))
                    .collect::<Result<_, serde_json::Error>>()?,
            ),
            })
        }
        convert(serde_json::to_value(value)?)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalyzerIdentity {
    pub name: String,
    pub version: String,
    pub configuration: Fact,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationInputs {
    pub values: Body,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub code: String,
    pub subjects: BTreeSet<EntryId>,
    pub facts: Fact,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FindingKey {
    pub database: Uuid,
    pub analyzer: AnalyzerIdentity,
    pub kind: MessageType,
    pub finding: Finding,
}

impl FindingKey {
    pub fn encoding(&self) -> Vec<u8> {
        //A No floats or unordered maps enter this versioned encoding. Equality
        //A uses the full typed key; the digest is only a display/index aid.
        let mut bytes = b"lifecore.finding.v1\0".to_vec();
        serde_json::to_writer(&mut bytes, self).expect("semantic key encoding");
        bytes
    }
    pub fn digest(&self) -> blake3::Hash {
        blake3::hash(&self.encoding())
    }
}
