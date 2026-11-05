use crate::{
    OffsetTimestamp, Timestamp, decimal,
    time::{parse_date, parse_duration},
};
use jiff::{SignedDuration, civil::Date};
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, IntoDeserializer, MapAccess, SeqAccess, Visitor},
    ser::SerializeStruct,
};
use std::{
    collections::BTreeMap,
    fmt,
    ops::{Deref, DerefMut},
};

// Field names to payload values. Its own `Deserialize` rejects repeated field
// names instead of keeping the last value, wherever a body appears.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct Body(BTreeMap<String, Payload>);

impl Body {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Deref for Body {
    type Target = BTreeMap<String, Payload>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Body {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<const N: usize> From<[(String, Payload); N]> for Body {
    fn from(fields: [(String, Payload); N]) -> Self {
        Self(BTreeMap::from(fields))
    }
}

impl From<BTreeMap<String, Payload>> for Body {
    fn from(fields: BTreeMap<String, Payload>) -> Self {
        Self(fields)
    }
}

impl FromIterator<(String, Payload)> for Body {
    fn from_iter<I: IntoIterator<Item = (String, Payload)>>(fields: I) -> Self {
        Self(fields.into_iter().collect())
    }
}

impl IntoIterator for Body {
    type Item = (String, Payload);
    type IntoIter = std::collections::btree_map::IntoIter<String, Payload>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a Body {
    type Item = (&'a String, &'a Payload);
    type IntoIter = std::collections::btree_map::Iter<'a, String, Payload>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'de> Deserialize<'de> for Body {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(BodyVisitor)
    }
}

// Canonical encoding: text and lists are native; every other kind is a
// `{kind, value}` envelope, integers carrying decimal strings and the time
// kinds their canonical strings, and a variant adds `tag`. Input also accepts
// bare i64 integers as shorthand. Equality and hashing are field by field
// on the parsed values, which the duplicate rule and the analysis recorder
// rely on: `PT120M` and `2h` are one duration, while one instant at two
// offsets is two offset timestamps.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Payload {
    Text(String),
    Integer(i64),
    Boolean(bool),
    Timestamp(Timestamp),
    Duration(SignedDuration),
    Date(Date),
    OffsetTimestamp(OffsetTimestamp),
    List(Vec<Payload>),
    Record(Body),
    Variant { tag: String, value: Body },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PayloadKind {
    Text,
    Integer,
    Boolean,
    Timestamp,
    Duration,
    Date,
    OffsetTimestamp,
    List,
    Record,
    Variant,
}

impl Payload {
    pub fn kind(&self) -> PayloadKind {
        match self {
        Self::Text(_) => PayloadKind::Text,
        Self::Integer(_) => PayloadKind::Integer,
        Self::Boolean(_) => PayloadKind::Boolean,
        Self::Timestamp(_) => PayloadKind::Timestamp,
        Self::Duration(_) => PayloadKind::Duration,
        Self::Date(_) => PayloadKind::Date,
        Self::OffsetTimestamp(_) => PayloadKind::OffsetTimestamp,
        Self::List(_) => PayloadKind::List,
        Self::Record(_) => PayloadKind::Record,
        Self::Variant { .. } => PayloadKind::Variant,
        }
    }
}

const TAGS: &[&str] = &[
    "integer",
    "boolean",
    "timestamp",
    "duration",
    "date",
    "offset_timestamp",
    "record",
    "variant",
];

impl Serialize for Payload {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        struct Decimal(i64);
        impl Serialize for Decimal {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                decimal::serialize(&self.0, serializer)
            }
        }
        // Jiff's `Display` is the canonical spelling: ISO 8601 for a
        // duration (`PT1H30M`), `YYYY-MM-DD` for a date.
        struct Displayed<'a, T>(&'a T);
        impl<T: fmt::Display> Serialize for Displayed<'_, T> {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(self.0)
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
        Self::Duration(value) => envelope(serializer, "duration", &Displayed(value)),
        Self::Date(value) => envelope(serializer, "date", &Displayed(value)),
        Self::OffsetTimestamp(value) => envelope(serializer, "offset_timestamp", value),
        Self::Record(fields) => envelope(serializer, "record", fields),
        Self::Variant { tag, value } => {
            let mut state = serializer.serialize_struct("Payload", 3)?;
            state.serialize_field("kind", "variant")?;
            state.serialize_field("tag", tag)?;
            state.serialize_field("value", value)?;
            state.end()
        }
        }
    }
}

impl<'de> Deserialize<'de> for Payload {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(PayloadVisitor)
    }
}

struct PayloadVisitor;

impl<'de> Visitor<'de> for PayloadVisitor {
    type Value = Payload;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(
            "a string, list, i64 integer, or tagged integer/boolean/timestamp/duration/date/offset_timestamp/record/variant",
        )
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
        // Object/table key order is not significant, so `value` may arrive
        // before `kind` and is buffered in a kind-neutral form.
        let mut kind: Option<String> = None;
        let mut tag: Option<String> = None;
        let mut value: Option<EnvelopeValue> = None;
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
            "kind" if kind.is_none() => kind = Some(map.next_value()?),
            "tag" if tag.is_none() => tag = Some(map.next_value()?),
            "value" if value.is_none() => value = Some(map.next_value()?),
            "kind" | "tag" | "value" => {
                return Err(A::Error::custom(format!("duplicate field `{key}`")));
            }
            other => return Err(A::Error::unknown_field(other, &["kind", "tag", "value"])),
            }
        }
        let kind = kind.ok_or_else(|| A::Error::missing_field("kind"))?;
        if !TAGS.contains(&kind.as_str()) {
            return Err(A::Error::unknown_variant(&kind, TAGS));
        }
        // Only a variant carries a tag; elsewhere it is an unknown field, so
        // `{kind = "record", tag = …}` cannot pass for a variant.
        match (kind.as_str(), &tag) {
        ("variant", None) => return Err(A::Error::missing_field("tag")),
        ("variant", Some(_)) => {}
        (_, Some(_)) => return Err(A::Error::unknown_field("tag", &["kind", "value"])),
        (_, None) => {}
        }
        let value = value.ok_or_else(|| A::Error::missing_field("value"))?;
        match (kind.as_str(), value) {
        ("integer", EnvelopeValue::Text(text)) => decimal::parse(&text)
            .map(Payload::Integer)
            .map_err(A::Error::custom),
        ("boolean", EnvelopeValue::Boolean(value)) => Ok(Payload::Boolean(value)),
        ("timestamp", EnvelopeValue::Text(text)) => {
            // Delegate to Jiff's own Serde parser; there is no LifeCore profile.
            Timestamp::deserialize(text.into_deserializer()).map(Payload::Timestamp)
        }
        ("duration", EnvelopeValue::Text(text)) => parse_duration(&text)
            .map(Payload::Duration)
            .map_err(A::Error::custom),
        ("date", EnvelopeValue::Text(text)) => parse_date(&text)
            .map(Payload::Date)
            .map_err(A::Error::custom),
        ("offset_timestamp", EnvelopeValue::Text(text)) => OffsetTimestamp::parse(&text)
            .map(Payload::OffsetTimestamp)
            .map_err(A::Error::custom),
        ("record", EnvelopeValue::Record(fields)) => Ok(Payload::Record(fields)),
        ("variant", EnvelopeValue::Record(value)) => Ok(Payload::Variant {
            tag: tag.expect("checked above"),
            value,
        }),
        (kind, value) => {
            let article = if kind.starts_with(['a', 'e', 'i', 'o', 'u']) {
                "an"
            } else {
                "a"
            };
            Err(A::Error::custom(format!(
                "{article} {kind} envelope cannot carry {}",
                value.describe()
            )))
        }
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
        // Serde sees a native TOML date or time as a map under this private
        // key, which the toml crate documents as unstable but has kept.
        Self::Record(fields)
            if fields.len() == 1 && fields.contains_key("$__toml_private_datetime") =>
        {
            "a native TOML date or time; quote the value as a string"
        }
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
                f.write_str("a decimal or time string, a boolean, or record fields")
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
        let mut body = BTreeMap::new();
        while let Some((key, value)) = map.next_entry::<String, Payload>()? {
            if body.contains_key(&key) {
                return Err(de::Error::custom(format!("duplicate field `{key}`")));
            }
            body.insert(key, value);
        }
        Ok(Body(body))
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
impl From<SignedDuration> for Payload {
    fn from(v: SignedDuration) -> Self {
        Self::Duration(v)
    }
}
impl From<Date> for Payload {
    fn from(v: Date) -> Self {
        Self::Date(v)
    }
}
impl From<OffsetTimestamp> for Payload {
    fn from(v: OffsetTimestamp) -> Self {
        Self::OffsetTimestamp(v)
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
