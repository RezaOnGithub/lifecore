use std::str::FromStr;

use anyhow::{Result, anyhow, bail};
use lifecore::{FieldQuery, NodeQueryPredicate, Payload, PayloadKind, QueryRegex};

pub fn predicate(source: &str) -> Result<NodeQueryPredicate> {
    if source == "fresh" {
        return Ok(NodeQueryPredicate::Fresh);
    }
    if let Some(value) = source.strip_prefix("type ") {
        return nonempty(value).map(|value| NodeQueryPredicate::NodeType(value.to_string()));
    }
    for (prefix, constructor) in [
        (
            "fresh-outgoing ",
            NodeQueryPredicate::HasFreshOutgoingEdgeOfType as fn(String) -> NodeQueryPredicate,
        ),
        (
            "fresh-incoming ",
            NodeQueryPredicate::HasFreshIncomingEdgeOfType,
        ),
        ("outgoing ", NodeQueryPredicate::HasOutgoingEdgeOfType),
        ("incoming ", NodeQueryPredicate::HasIncomingEdgeOfType),
    ] {
        if let Some(value) = source.strip_prefix(prefix) {
            return nonempty(value).map(|value| constructor(value.to_string()));
        }
    }
    if let Some(inner) = source
        .strip_prefix("not (")
        .and_then(|value| value.strip_suffix(')'))
    {
        return Ok(NodeQueryPredicate::Not(Box::new(predicate(inner.trim())?)));
    }
    let remainder = source
        .strip_prefix("field ")
        .ok_or_else(|| anyhow!("unknown query predicate {source:?}"))?;
    let mut pieces = remainder.splitn(3, ' ');
    let path = pieces
        .next()
        .ok_or_else(|| anyhow!("field predicate is missing a path"))?;
    let operation = pieces
        .next()
        .ok_or_else(|| anyhow!("field predicate is missing an operation"))?;
    let operand = pieces.next();
    let path = FieldQuery::from_str(path).map_err(|_| anyhow!("invalid field path"))?;
    match operation {
    "exists" if operand.is_none() => Ok(NodeQueryPredicate::FieldExists(path)),
    "kind" => Ok(NodeQueryPredicate::FieldKind(
        path,
        payload_kind(required(operand)?)?,
    )),
    "equals-text" => Ok(NodeQueryPredicate::FieldEqual(
        path,
        Payload::Text(required(operand)?.to_string()),
    )),
    "equals-number" => Ok(NodeQueryPredicate::FieldEqual(
        path,
        Payload::Number(integer(required(operand)?)?),
    )),
    "matches" => Ok(NodeQueryPredicate::FieldIsStringAndMatches(
        path,
        QueryRegex::new(required(operand)?)?,
    )),
    "sequence-contains-text" => Ok(NodeQueryPredicate::FieldIsSequenceAndContains(
        path,
        Payload::Text(required(operand)?.to_string()),
    )),
    "sequence-contains-number" => Ok(NodeQueryPredicate::FieldIsSequenceAndContains(
        path,
        Payload::Number(integer(required(operand)?)?),
    )),
    "sequence-count" => Ok(NodeQueryPredicate::FieldIsSequenceAndElementCountIs(
        path,
        natural(required(operand)?)?,
    )),
    "set-contains-text" => Ok(NodeQueryPredicate::FieldIsOrderedSetAndContains(
        path,
        Payload::Text(required(operand)?.to_string()),
    )),
    "set-contains-number" => Ok(NodeQueryPredicate::FieldIsOrderedSetAndContains(
        path,
        Payload::Number(integer(required(operand)?)?),
    )),
    "set-count" => Ok(NodeQueryPredicate::FieldIsOrderedSetAndElementCountIs(
        path,
        natural(required(operand)?)?,
    )),
    "count" => Ok(NodeQueryPredicate::FieldQueryResultCount(
        path,
        natural(required(operand)?)?,
    )),
    _ => bail!("invalid field predicate operation or operand"),
    }
}

fn required(value: Option<&str>) -> Result<&str> {
    value
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("query predicate is missing an operand"))
}

fn nonempty(value: &str) -> Result<&str> {
    required(Some(value))
}

fn integer(value: &str) -> Result<i64> {
    if value.starts_with('+') || value.trim() != value {
        bail!("number predicate requires a signed decimal integer");
    }
    value
        .parse()
        .map_err(|_| anyhow!("number predicate requires a signed decimal integer"))
}

fn natural(value: &str) -> Result<usize> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!("count predicate requires a natural number");
    }
    value
        .parse()
        .map_err(|_| anyhow!("count predicate is too large"))
}

fn payload_kind(value: &str) -> Result<PayloadKind> {
    match value {
    "PreciseTime" | "precisetime" => Ok(PayloadKind::PreciseTime),
    "Text" | "text" => Ok(PayloadKind::Text),
    "Number" | "number" => Ok(PayloadKind::Number),
    "Sequence" | "sequence" => Ok(PayloadKind::Sequence),
    "OrderedSet" | "orderedset" | "ordered_set" => Ok(PayloadKind::OrderedSet),
    _ => bail!("unknown payload kind {value:?}"),
    }
}
