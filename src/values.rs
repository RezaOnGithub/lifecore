use crate::{Timestamp, decimal};
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, IntoDeserializer, MapAccess, SeqAccess, Visitor},
    ser::SerializeStruct,
};
use std::{collections::BTreeMap, fmt};

pub type Body = BTreeMap<String, Payload>;

//A Canonical encoding: text and lists are native; every other kind is a
//A `{kind, value}` envelope, integers carrying decimal strings. Input also
//A accepts bare i64 integers as shorthand.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Payload {
    Text(String),
    Integer(i64),
    Boolean(bool),
    Timestamp(Timestamp),
    List(Vec<Payload>),
    Record(Body),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PayloadKind {
    Text,
    Integer,
    Boolean,
    Timestamp,
    List,
    Record,
}

impl Payload {
    pub fn kind(&self) -> PayloadKind {
        match self {
        Self::Text(_) => PayloadKind::Text,
        Self::Integer(_) => PayloadKind::Integer,
        Self::Boolean(_) => PayloadKind::Boolean,
        Self::Timestamp(_) => PayloadKind::Timestamp,
        Self::List(_) => PayloadKind::List,
        Self::Record(_) => PayloadKind::Record,
        }
    }
}

const TAGS: &[&str] = &["integer", "boolean", "timestamp", "record"];

impl Serialize for Payload {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        struct Decimal(i64);
        impl Serialize for Decimal {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                decimal::serialize(&self.0, serializer)
            }
        }
        fn envelope<S: Serializer, T: Serialize>(
            serializer: S,
            kind: &'static str,
            value: &T,
        ) -> Result<S::Ok, S::Error> {
            let mut state = serializer.serialize_struct("Payload", 2)?;
            state.serialize_field("kind", kind)?;
            state.serialize_field("value", value)?;
            state.end()
        }
        match self {
        Self::Text(text) => serializer.serialize_str(text),
        Self::List(values) => values.serialize(serializer),
        Self::Integer(value) => envelope(serializer, "integer", &Decimal(*value)),
        Self::Boolean(value) => envelope(serializer, "boolean", value),
        Self::Timestamp(value) => envelope(serializer, "timestamp", value),
        Self::Record(fields) => envelope(serializer, "record", fields),
        }
    }
}

impl<'de> Deserialize<'de> for Payload {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(PayloadVisitor)
    }
}

//A Records reject repeated field names instead of keeping the last value.
pub fn deserialize_body<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Body, D::Error> {
    deserializer.deserialize_map(BodyVisitor)
}

struct PayloadVisitor;

impl<'de> Visitor<'de> for PayloadVisitor {
    type Value = Payload;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a string, list, i64 integer, or tagged integer/boolean/timestamp/record")
    }
    fn visit_str<E: de::Error>(self, value: &str) -> Result<Payload, E> {
        Ok(Payload::Text(value.into()))
    }
    fn visit_string<E: de::Error>(self, value: String) -> Result<Payload, E> {
        Ok(Payload::Text(value))
    }
    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Payload, E> {
        Ok(Payload::Integer(value))
    }
    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Payload, E> {
        i64::try_from(value)
            .map(Payload::Integer)
            .map_err(|_| E::custom(format!("integer {value} is outside the i64 range")))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Payload, A::Error> {
        let mut values = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(1024));
        while let Some(value) = seq.next_element()? {
            values.push(value);
        }
        Ok(Payload::List(values))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Payload, A::Error> {
        use de::Error;
        //A Object/table key order is not significant, so `value` may arrive
        //A before `kind` and is buffered in a kind-neutral form.
        let mut kind: Option<String> = None;
        let mut value: Option<EnvelopeValue> = None;
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
            "kind" if kind.is_none() => kind = Some(map.next_value()?),
            "value" if value.is_none() => value = Some(map.next_value()?),
            "kind" | "value" => return Err(A::Error::custom(format!("duplicate field `{key}`"))),
            other => return Err(A::Error::unknown_field(other, &["kind", "value"])),
            }
        }
        let kind = kind.ok_or_else(|| A::Error::missing_field("kind"))?;
        let value = value.ok_or_else(|| A::Error::missing_field("value"))?;
        match (kind.as_str(), value) {
        ("integer", EnvelopeValue::Text(text)) => decimal::parse(&text)
            .map(Payload::Integer)
            .map_err(A::Error::custom),
        ("boolean", EnvelopeValue::Boolean(value)) => Ok(Payload::Boolean(value)),
        ("timestamp", EnvelopeValue::Text(text)) => {
            //A Delegate to Jiff's own Serde parser; there is no LifeCore profile.
            Timestamp::deserialize(text.into_deserializer()).map(Payload::Timestamp)
        }
        ("record", EnvelopeValue::Record(fields)) => Ok(Payload::Record(fields)),
        (kind, value) if TAGS.contains(&kind) => Err(A::Error::custom(format!(
            "a {kind} envelope cannot carry {}",
            value.describe()
        ))),
        (kind, _) => Err(A::Error::unknown_variant(kind, TAGS)),
        }
    }
}

enum EnvelopeValue {
    Text(String),
    Boolean(bool),
    Record(Body),
}

impl EnvelopeValue {
    fn describe(&self) -> &'static str {
        match self {
        Self::Text(_) => "a string",
        Self::Boolean(_) => "a boolean",
        Self::Record(_) => "a record",
        }
    }
}

impl<'de> Deserialize<'de> for EnvelopeValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct EnvelopeVisitor;
        impl<'de> Visitor<'de> for EnvelopeVisitor {
            type Value = EnvelopeValue;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a decimal or timestamp string, a boolean, or record fields")
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<EnvelopeValue, E> {
                Ok(EnvelopeValue::Text(value.into()))
            }
            fn visit_string<E: de::Error>(self, value: String) -> Result<EnvelopeValue, E> {
                Ok(EnvelopeValue::Text(value))
            }
            fn visit_bool<E: de::Error>(self, value: bool) -> Result<EnvelopeValue, E> {
                Ok(EnvelopeValue::Boolean(value))
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<EnvelopeValue, A::Error> {
                BodyVisitor.visit_map(map).map(EnvelopeValue::Record)
            }
        }
        deserializer.deserialize_any(EnvelopeVisitor)
    }
}

struct BodyVisitor;

impl<'de> Visitor<'de> for BodyVisitor {
    type Value = Body;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a map of field names to payload values")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Body, A::Error> {
        let mut body = Body::new();
        while let Some((key, value)) = map.next_entry::<String, Payload>()? {
            if body.contains_key(&key) {
                return Err(de::Error::custom(format!("duplicate field `{key}`")));
            }
            body.insert(key, value);
        }
        Ok(body)
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
        Self::Integer(v)
    }
}
impl From<bool> for Payload {
    fn from(v: bool) -> Self {
        Self::Boolean(v)
    }
}
impl From<Timestamp> for Payload {
    fn from(v: Timestamp) -> Self {
        Self::Timestamp(v)
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
