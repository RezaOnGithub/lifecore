use std::{cmp::Ordering, io::IsTerminal};

use clap::ValueEnum;
use lifecore::{
    BasicValidationFailure, DiagnosticLocation, Body, PointedEventContext,
    analysis::{Message, SuggestedAction},
};
use serde::Serialize;
use serde_json::{Value, json};
use sha3::{Digest, Sha3_512};

use crate::{state::Barklist, util::pointed_index};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    #[default]
    Auto,
    Table,
    Lines,
    Json,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActionRecord {
    AddNode {
        ty: String,
        body: Body,
        parent: Option<PointedEventContext>,
    },
    AddEdge {
        from: PointedEventContext,
        ty: String,
        to: PointedEventContext,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct DiagnosticRecord {
    pub index: usize,
    pub severity: &'static str,
    pub offending_event: Value,
    pub system: String,
    pub config: String,
    pub message: String,
    pub relevance: Vec<PointedEventContext>,
    pub actions: Vec<ActionRecord>,
    pub hash: String,
}

fn location_value(location: DiagnosticLocation) -> Value {
    match location {
    DiagnosticLocation::Window(window) => json!({ "window": window }),
    DiagnosticLocation::Event(event) => json!({ "event": event }),
    }
}

fn canonical_window_value(window: lifecore::EventContext) -> Value {
    let end = serde_json::to_value(window)
        .expect("EventContext must serialize")
        .get("end")
        .and_then(Value::as_u64)
        .expect("EventContext serialization must contain end") as usize;
    if end == usize::MAX {
        Value::String("universe".to_string())
    } else {
        json!(end)
    }
}

fn canonical_pointed_value(context: PointedEventContext) -> Value {
    json!({
        "end": canonical_window_value(context.window()),
        "thing": pointed_index(context),
    })
}

fn canonical_location_value(location: DiagnosticLocation) -> Value {
    match location {
    DiagnosticLocation::Window(window) => json!({ "window": canonical_window_value(window) }),
    DiagnosticLocation::Event(event) => json!({ "event": canonical_pointed_value(event) }),
    }
}

fn canonical_action_value(action: &SuggestedAction) -> Value {
    match action {
    SuggestedAction::AddNode {
        ty_name,
        payload,
        parent,
    } => json!({
        "kind": "add_node",
        "type": ty_name,
        "body": payload,
        "parent": parent.map(canonical_pointed_value),
    }),
    SuggestedAction::AddEdge { from, ty_name, to } => json!({
        "kind": "add_edge",
        "from": canonical_pointed_value(*from),
        "type": ty_name,
        "to": canonical_pointed_value(*to),
    }),
    }
}

fn action_record(action: &SuggestedAction) -> ActionRecord {
    match action {
    SuggestedAction::AddNode {
        ty_name,
        payload,
        parent,
    } => ActionRecord::AddNode {
        ty: ty_name.clone(),
        body: payload.clone(),
        parent: *parent,
    },
    SuggestedAction::AddEdge { from, ty_name, to } => ActionRecord::AddEdge {
        from: *from,
        ty: ty_name.clone(),
        to: *to,
    },
    }
}

fn parts(
    message: &Message,
) -> (
    &'static str,
    DiagnosticLocation,
    &str,
    &str,
    &str,
    Vec<PointedEventContext>,
    Vec<ActionRecord>,
) {
    match message {
    Message::Inconsistency(value) => (
        "inconsistency",
        value.offending_event,
        &value.system_name,
        &value.system_config,
        &value.message,
        Vec::new(),
        Vec::new(),
    ),
    Message::Warning(value) => (
        "warning",
        value.offending_event,
        &value.system_name,
        &value.system_config,
        &value.message,
        value.relevance.clone(),
        Vec::new(),
    ),
    Message::Suggestion(value) => (
        "suggestion",
        value.offending_event,
        &value.system_name,
        &value.system_config,
        &value.message,
        value.relevance.clone(),
        value.action.iter().map(action_record).collect(),
    ),
    }
}

pub fn message_hash(message: &Message) -> String {
    let (severity, location, system, config, text, mut relevance, _) = parts(message);
    relevance.sort();
    let actions = match message {
    Message::Suggestion(suggestion) => suggestion
        .action
        .iter()
        .map(canonical_action_value)
        .collect::<Vec<_>>(),
    Message::Inconsistency(_) | Message::Warning(_) => Vec::new(),
    };
    let canonical = json!({
        "severity": severity,
        "offending_event": canonical_location_value(location),
        "system": system,
        "config": config,
        "message": text,
        "relevance": relevance.into_iter().map(canonical_pointed_value).collect::<Vec<_>>(),
        "actions": actions,
    });
    let bytes = serde_json::to_vec(&canonical).expect("diagnostic canonical form must serialize");
    format!("{:x}", Sha3_512::digest(bytes))
}

fn location_key(location: DiagnosticLocation) -> (usize, u8, usize) {
    match location {
    DiagnosticLocation::Event(event) => (pointed_index(event), 0, pointed_index(event)),
    DiagnosticLocation::Window(window) => {
        let end = serde_json::to_value(window)
            .expect("EventContext must serialize")
            .get("end")
            .and_then(Value::as_u64)
            .expect("EventContext serialization must contain end") as usize;
        (end.saturating_sub(1), 1, end)
    }
    }
}

fn severity_rank(message: &Message) -> u8 {
    match message {
    Message::Inconsistency(_) => 0,
    Message::Warning(_) => 1,
    Message::Suggestion(_) => 2,
    }
}

pub fn sort_messages(messages: &mut [Message]) {
    messages.sort_by(|left, right| {
        let (_, left_location, left_system, left_config, left_text, _, _) = parts(left);
        let (_, right_location, right_system, right_config, right_text, _, _) = parts(right);
        location_key(left_location)
            .cmp(&location_key(right_location))
            .then_with(|| severity_rank(left).cmp(&severity_rank(right)))
            .then_with(|| left_system.cmp(right_system))
            .then_with(|| left_config.cmp(right_config))
            .then_with(|| left_text.cmp(right_text))
            .then_with(|| message_hash(left).cmp(&message_hash(right)))
    });
}

fn context_is_resolved(context: PointedEventContext, barklist: &Barklist) -> bool {
    barklist
        .resolved
        .iter()
        .any(|resolved| resolved.absolute() == context.absolute())
}

pub fn is_barked(message: &Message, barklist: &Barklist) -> bool {
    if matches!(message, Message::Inconsistency(_)) {
        return false;
    }
    if barklist
        .hashes
        .iter()
        .any(|hash| hash == &message_hash(message))
    {
        return true;
    }
    let (_, offending, _, _, _, relevance, _) = parts(message);
    match offending {
    DiagnosticLocation::Window(_) => false,
    DiagnosticLocation::Event(event) => {
        context_is_resolved(event, barklist)
            && relevance
                .iter()
                .all(|context| context_is_resolved(*context, barklist))
    }
    }
}

pub fn visible_messages(
    messages: impl IntoIterator<Item = Message>,
    barklist: &Barklist,
    no_bark: bool,
) -> Vec<Message> {
    let mut messages = messages
        .into_iter()
        .filter(|message| no_bark || !is_barked(message, barklist))
        .collect::<Vec<_>>();
    sort_messages(&mut messages);
    messages
}

pub fn records(messages: &[Message]) -> Vec<DiagnosticRecord> {
    messages
        .iter()
        .enumerate()
        .map(|(index, message)| {
            let (severity, location, system, config, text, relevance, actions) = parts(message);
            DiagnosticRecord {
                index: index + 1,
                severity,
                offending_event: location_value(location),
                system: system.to_string(),
                config: config.to_string(),
                message: text.to_string(),
                relevance,
                actions,
                hash: message_hash(message),
            }
        })
        .collect()
}

pub fn print_diagnostics(messages: &[Message], format: OutputFormat) -> anyhow::Result<()> {
    let records = records(messages);
    if matches!(resolved_format(format), OutputFormat::Json) {
        println!("{}", serde_json::to_string_pretty(&records)?);
        return Ok(());
    }
    if records.is_empty() {
        println!("No diagnostics.");
        return Ok(());
    }

    match resolved_format(format) {
    OutputFormat::Lines => {
        for record in records {
            println!(
                "{}\t{}\t{}\t{}\t{}",
                record.index,
                record.severity,
                compact_json(&record.offending_event),
                record.hash,
                record.message
            );
        }
    }
    OutputFormat::Table => {
        let rows = records
            .iter()
            .map(|record| {
                vec![
                    record.index.to_string(),
                    record.severity.to_string(),
                    compact_json(&record.offending_event),
                    record.hash.chars().take(12).collect(),
                    record.message.clone(),
                ]
            })
            .collect::<Vec<_>>();
        print_table(&["#", "severity", "location", "hash", "message"], &rows);
    }
    OutputFormat::Auto | OutputFormat::Json => unreachable!(),
    }
    Ok(())
}

pub fn print_validation_failure(
    failure: &BasicValidationFailure,
    format: OutputFormat,
) -> anyhow::Result<()> {
    let value = json!({
        "index": 1,
        "severity": "basic_validation_failure",
        "offending_event": location_value(failure.offending_event),
        "reason": format!("{:?}", failure.reason),
    });
    if matches!(resolved_format(format), OutputFormat::Json) {
        println!("{}", serde_json::to_string_pretty(&vec![value])?);
    } else {
        println!(
            "basic validation failure at {}: {:?}",
            compact_json(&location_value(failure.offending_event)),
            failure.reason
        );
    }
    Ok(())
}

pub fn resolved_format(format: OutputFormat) -> OutputFormat {
    match format {
    OutputFormat::Auto if std::io::stdout().is_terminal() => OutputFormat::Table,
    OutputFormat::Auto => OutputFormat::Lines,
    other => other,
    }
}

pub fn compact_json(value: &Value) -> String {
    serde_json::to_string(value).expect("JSON value must serialize")
}

pub fn print_rows(
    headers: &[&str],
    rows: &[Vec<String>],
    json_value: &Value,
    format: OutputFormat,
) -> anyhow::Result<()> {
    match resolved_format(format) {
    OutputFormat::Json => println!("{}", serde_json::to_string_pretty(json_value)?),
    OutputFormat::Table => print_table(headers, rows),
    OutputFormat::Lines => {
        for row in rows {
            println!("{}", row.join("\t"));
        }
    }
    OutputFormat::Auto => unreachable!(),
    }
    Ok(())
}

fn print_table(headers: &[&str], rows: &[Vec<String>]) {
    let terminal_width = std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|width| *width >= 40)
        .unwrap_or(120);
    let mut widths = headers
        .iter()
        .map(|header| header.len())
        .collect::<Vec<_>>();
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(cell.chars().count());
        }
    }
    let separators = widths.len().saturating_sub(1) * 3;
    let fixed = widths
        .iter()
        .take(widths.len().saturating_sub(1))
        .sum::<usize>()
        + separators;
    if let Some(last) = widths.last_mut() {
        *last = (*last).min(terminal_width.saturating_sub(fixed).max(12));
    }

    let render = |cells: &[String]| {
        cells
            .iter()
            .enumerate()
            .map(|(index, cell)| fit(cell, widths[index]))
            .collect::<Vec<_>>()
            .join(" | ")
    };
    println!(
        "{}",
        render(
            &headers
                .iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
        )
    );
    println!(
        "{}",
        widths
            .iter()
            .map(|width| "-".repeat(*width))
            .collect::<Vec<_>>()
            .join("-+-")
    );
    for row in rows {
        println!("{}", render(row));
    }
}

fn fit(value: &str, width: usize) -> String {
    let count = value.chars().count();
    match count.cmp(&width) {
    Ordering::Less => format!("{value}{}", " ".repeat(width - count)),
    Ordering::Equal => value.to_string(),
    Ordering::Greater if width <= 1 => "…".to_string(),
    Ordering::Greater => format!("{}…", value.chars().take(width - 1).collect::<String>()),
    }
}

#[cfg(test)]
mod tests {
    use lifecore::{DiagnosticLocation, PointedEventContext, analysis::Warning};

    use super::*;

    fn warning(relevance: Vec<PointedEventContext>) -> Message {
        Warning {
            offending_event: DiagnosticLocation::Event(PointedEventContext::new(3)),
            system_name: "test".into(),
            system_config: "config".into(),
            relevance,
            message: "hello".into(),
        }
        .into()
    }

    #[test]
    fn canonical_hash_ignores_relevance_order() {
        let a = PointedEventContext::new(1);
        let b = PointedEventContext::new(2);
        assert_eq!(
            message_hash(&warning(vec![a, b])),
            message_hash(&warning(vec![b, a]))
        );
    }

    #[test]
    fn resolved_event_and_relevance_suppress_warning() {
        let event = PointedEventContext::new(3);
        let relevant = PointedEventContext::new(2);
        let barklist = Barklist {
            hashes: vec![],
            resolved: vec![event, relevant],
        };
        assert!(is_barked(&warning(vec![relevant]), &barklist));
    }
}
