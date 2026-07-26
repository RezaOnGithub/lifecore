use lifecore::{
    FieldQuery, LifeCore, LifeCoreList, Node, NodeQuery, NodeQueryExplanation, NodeQueryExpression,
    NodeQueryPredicate, Payload, PayloadKind, QueryRegex,
};

use crate::{cli::QueryCombinator, error::AppError, view::GraphView};

#[derive(Clone)]
pub struct ParsedNodeQuery {
    pub query: NodeQuery,
    pub predicates: Vec<String>,
    pub combinator: QueryCombinator,
}

pub fn parse_query(
    view: &GraphView,
    combinator: QueryCombinator,
    predicates: &[String],
) -> Result<ParsedNodeQuery, AppError> {
    //A Preserve source predicate order beside the core expression so
    //A explanations can pair core booleans with the exact user input.
    let parsed = predicates
        .iter()
        .map(|predicate| parse_predicate(predicate))
        .collect::<Result<Vec<_>, _>>()?;
    let expression = match combinator {
    QueryCombinator::All => NodeQueryExpression::All(parsed),
    QueryCombinator::Any => NodeQueryExpression::Any(parsed),
    QueryCombinator::None => NodeQueryExpression::None(parsed),
    };

    Ok(ParsedNodeQuery {
        query: NodeQuery {
            context: view.context(),
            expression,
        },
        predicates: predicates.to_vec(),
        combinator,
    })
}

pub fn execute_query(graph: &LifeCoreList, query: &ParsedNodeQuery) -> Vec<Node> {
    graph.query(query.query.clone())
}

pub fn explain_query(
    graph: &LifeCoreList,
    query: &ParsedNodeQuery,
    candidate: lifecore::EntryContext,
) -> Result<NodeQueryExplanation, AppError> {
    graph.explain_query(&query.query, candidate).ok_or_else(|| {
        AppError::invalid_state("query candidate is not a node in the selected view")
    })
}

pub fn parse_predicate(text: &str) -> Result<NodeQueryPredicate, AppError> {
    parse_predicate_inner(text.trim())
        .map_err(|message| AppError::invalid_query_predicate(text.to_string(), message))
}

fn parse_predicate_inner(text: &str) -> Result<NodeQueryPredicate, String> {
    if text == "fresh" {
        return Ok(NodeQueryPredicate::Fresh);
    }
    if let Some(inner) = text
        .strip_prefix("not (")
        .and_then(|rest| rest.strip_suffix(')'))
    {
        return parse_predicate_inner(inner.trim())
            .map(|predicate| NodeQueryPredicate::Not(Box::new(predicate)));
    }

    for (prefix, constructor) in [
        (
            "type ",
            NodeQueryPredicate::NodeType as fn(String) -> NodeQueryPredicate,
        ),
        ("outgoing ", NodeQueryPredicate::HasOutgoingEdgeOfType),
        ("incoming ", NodeQueryPredicate::HasIncomingEdgeOfType),
        (
            "fresh-outgoing ",
            NodeQueryPredicate::HasFreshOutgoingEdgeOfType,
        ),
        (
            "fresh-incoming ",
            NodeQueryPredicate::HasFreshIncomingEdgeOfType,
        ),
    ] {
        if let Some(value) = text.strip_prefix(prefix) {
            if value.is_empty() || value.chars().any(char::is_whitespace) {
                return Err(format!("{prefix:?} requires one non-empty name"));
            }

            return Ok(constructor(value.to_string()));
        }
    }

    let Some(field_expression) = text.strip_prefix("field ") else {
        return Err("unknown predicate form".to_string());
    };
    let (path_text, operation) = split_once_whitespace(field_expression)
        .ok_or_else(|| "field predicate requires a path and operation".to_string())?;
    let path = path_text
        .parse::<FieldQuery>()
        .map_err(|_| "invalid field path".to_string())?;

    if operation == "exists" {
        return Ok(NodeQueryPredicate::FieldExists(path));
    }

    let (operator, value) = split_once_whitespace(operation)
        .ok_or_else(|| "field operation requires a value".to_string())?;
    match operator {
    "kind" => parse_payload_kind(value).map(|kind| NodeQueryPredicate::FieldKind(path, kind)),
    "equals-text" => Ok(NodeQueryPredicate::FieldEqual(
        path,
        Payload::Text(value.to_string()),
    )),
    "equals-number" => parse_integer(value)
        .map(|number| NodeQueryPredicate::FieldEqual(path, Payload::Number(number))),
    "matches" => QueryRegex::new(value)
        .map(|regex| NodeQueryPredicate::FieldIsStringAndMatches(path, regex))
        .map_err(|error| format!("invalid regular expression: {error}")),
    "sequence-contains-text" => Ok(NodeQueryPredicate::FieldIsSequenceAndContains(
        path,
        Payload::Text(value.to_string()),
    )),
    "sequence-contains-number" => parse_integer(value).map(|number| {
        NodeQueryPredicate::FieldIsSequenceAndContains(path, Payload::Number(number))
    }),
    "sequence-count" => parse_natural(value)
        .map(|count| NodeQueryPredicate::FieldIsSequenceAndElementCountIs(path, count)),
    "set-contains-text" => Ok(NodeQueryPredicate::FieldIsOrderedSetAndContains(
        path,
        Payload::Text(value.to_string()),
    )),
    "set-contains-number" => parse_integer(value).map(|number| {
        NodeQueryPredicate::FieldIsOrderedSetAndContains(path, Payload::Number(number))
    }),
    "set-count" => parse_natural(value)
        .map(|count| NodeQueryPredicate::FieldIsOrderedSetAndElementCountIs(path, count)),
    "count" => {
        parse_natural(value).map(|count| NodeQueryPredicate::FieldQueryResultCount(path, count))
    }
    _ => Err(format!("unknown field operation {operator:?}")),
    }
}

fn split_once_whitespace(text: &str) -> Option<(&str, &str)> {
    let index = text.find(char::is_whitespace)?;
    let left = &text[..index];
    let right = text[index..].trim_start();
    (!left.is_empty() && !right.is_empty()).then_some((left, right))
}

fn parse_integer(text: &str) -> Result<i64, String> {
    if text.is_empty()
        || text
            .strip_prefix('-')
            .unwrap_or(text)
            .bytes()
            .any(|byte| !byte.is_ascii_digit())
    {
        return Err("expected an integer".to_string());
    }

    text.parse::<i64>()
        .map_err(|_| "integer is out of range".to_string())
}

fn parse_natural(text: &str) -> Result<usize, String> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("expected a non-negative integer".to_string());
    }

    text.parse::<usize>()
        .map_err(|_| "count is out of range".to_string())
}

fn parse_payload_kind(text: &str) -> Result<PayloadKind, String> {
    match text {
    "precise-time" | "PreciseTime" => Ok(PayloadKind::PreciseTime),
    "text" | "Text" => Ok(PayloadKind::Text),
    "number" | "Number" => Ok(PayloadKind::Number),
    "sequence" | "Sequence" => Ok(PayloadKind::Sequence),
    "ordered-set" | "OrderedSet" => Ok(PayloadKind::OrderedSet),
    _ => Err(format!("unknown payload kind {text:?}")),
    }
}
