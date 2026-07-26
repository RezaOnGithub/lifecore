use lifecore::{Body, OrderedSet, Payload};

use crate::error::AppError;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FieldAssignment {
    pub name: String,
    pub value: Payload,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PayloadInput {
    Text(String),
    Number(i64),
    Sequence(Vec<PayloadInput>),
    OrderedSet(Vec<PayloadInput>),
}

impl PayloadInput {
    pub(crate) fn into_payload(self) -> Result<Payload, AppError> {
        match self {
        Self::Text(value) => Ok(Payload::Text(value)),
        Self::Number(value) => Ok(Payload::Number(value)),
        Self::Sequence(values) => values
            .into_iter()
            .map(Self::into_payload)
            .collect::<Result<Vec<_>, _>>()
            .map(Payload::Sequence),
        Self::OrderedSet(values) => {
            let mut result = OrderedSet::new();
            for value in values {
                if !result.insert(value.into_payload()?) {
                    return Err(AppError::invalid_payload(
                        "ordered_set contains a duplicate element",
                    ));
                }
            }

            Ok(Payload::OrderedSet(result))
        }
        }
    }
}

pub fn parse_field_assignment(text: &str) -> Result<FieldAssignment, AppError> {
    let table: toml::Table = toml::from_str(text)
        .map_err(|error| AppError::invalid_field_assignment(error.to_string()))?;
    if table.len() != 1 {
        return Err(AppError::invalid_field_assignment(
            "expected a TOML document with exactly one root key",
        ));
    }

    let (name, value) = table
        .into_iter()
        .next()
        .ok_or_else(|| AppError::invalid_field_assignment("missing root key"))?;
    let value = payload_from_toml(&value)?.into_payload()?;

    Ok(FieldAssignment { name, value })
}

pub fn parse_field_assignments(values: &[String]) -> Result<Body, AppError> {
    let mut body = Body::new();
    for value in values {
        let assignment = parse_field_assignment(value)?;
        if body
            .insert(assignment.name.clone(), assignment.value)
            .is_some()
        {
            return Err(AppError::invalid_field_assignment(format!(
                "duplicate field name {:?}",
                assignment.name
            )));
        }
    }

    Ok(body)
}

pub fn payload_from_toml(value: &toml::Value) -> Result<PayloadInput, AppError> {
    match value {
    toml::Value::String(value) => Ok(PayloadInput::Text(value.clone())),
    toml::Value::Integer(value) => Ok(PayloadInput::Number(*value)),
    toml::Value::Table(table) if table.len() == 1 => {
        let (constructor, values) = table
            .iter()
            .next()
            .ok_or_else(|| AppError::invalid_payload("missing payload constructor"))?;
        let values = values.as_array().ok_or_else(|| {
            AppError::invalid_payload(format!(
                "payload constructor {constructor:?} requires an array"
            ))
        })?;
        let values = values
            .iter()
            .map(payload_from_toml)
            .collect::<Result<Vec<_>, _>>()?;

        match constructor.as_str() {
        "sequence" => Ok(PayloadInput::Sequence(values)),
        "ordered_set" => Ok(PayloadInput::OrderedSet(values)),
        _ => Err(AppError::invalid_payload(format!(
            "unknown payload constructor {constructor:?}"
        ))),
        }
    }
    toml::Value::Table(_) => Err(AppError::invalid_payload(
        "payload constructor table must contain exactly one key",
    )),
    toml::Value::Array(_) => Err(AppError::invalid_payload(
        "bare TOML arrays are not payloads",
    )),
    toml::Value::Boolean(_) => Err(AppError::invalid_payload(
        "TOML booleans are not supported payloads",
    )),
    toml::Value::Float(_) => Err(AppError::invalid_payload(
        "TOML floats are not supported payloads",
    )),
    toml::Value::Datetime(_) => Err(AppError::invalid_payload(
        "TOML date-time values are not supported payloads",
    )),
    }
}
