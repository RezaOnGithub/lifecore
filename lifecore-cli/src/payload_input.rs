use anyhow::{Result, anyhow, bail};
use lifecore::{Body, OrderedSet, Payload};

pub fn assignments(values: &[String]) -> Result<Body> {
    let mut body = Body::new();
    for source in values {
        let table: toml::Table =
            toml::from_str(source).map_err(|error| anyhow!("invalid field assignment: {error}"))?;
        if table.len() != 1 {
            bail!("field assignment must be a TOML document with exactly one root key");
        }
        let (name, value) = table.into_iter().next().unwrap();
        if body.contains_key(&name) {
            bail!("duplicate field assignment {name:?}");
        }
        body.insert(name, convert(&value)?);
    }
    Ok(body)
}

pub fn convert(value: &toml::Value) -> Result<Payload> {
    match value {
    toml::Value::String(value) => Ok(Payload::Text(value.clone())),
    toml::Value::Integer(value) => Ok(Payload::Number(*value)),
    toml::Value::Table(table) if table.len() == 1 => {
        let (constructor, values) = table.iter().next().unwrap();
        let values = values
            .as_array()
            .ok_or_else(|| anyhow!("{constructor} constructor requires an array"))?;
        match constructor.as_str() {
        "sequence" => Ok(Payload::Sequence(
            values.iter().map(convert).collect::<Result<_>>()?,
        )),
        "ordered_set" => {
            let mut result = OrderedSet::new();
            for value in values {
                let value = convert(value)?;
                if !result.insert(value) {
                    bail!("ordered_set contains a duplicate element");
                }
            }
            Ok(Payload::OrderedSet(result))
        }
        _ => bail!("unknown payload constructor {constructor:?}"),
        }
    }
    toml::Value::Table(_) => {
        bail!("payload constructor table must contain exactly one key")
    }
    toml::Value::Array(_) => bail!("bare TOML arrays are not payloads"),
    toml::Value::Boolean(_) => bail!("TOML booleans are not supported payloads"),
    toml::Value::Float(_) => bail!("TOML floats are not supported payloads"),
    toml::Value::Datetime(_) => bail!("TOML date-time values are not supported payloads"),
    }
}

pub fn body(table: &toml::Table) -> Result<Body> {
    table
        .iter()
        .map(|(name, value)| Ok((name.clone(), convert(value)?)))
        .collect()
}
