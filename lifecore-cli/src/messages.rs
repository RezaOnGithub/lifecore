use std::collections::BTreeMap;

use anyhow::{Result, anyhow, bail};
use lifecore::{
    Body, Context, DiagnosticLocation, EntryIndex, LifeCore, LifeCoreList, Payload,
    analysis::{Message, SuggestedAction},
};
use serde::Serialize;
use sha3::{Digest, Sha3_512};

use crate::state::{Dismissal, MESSAGE_ID_VERSION};

#[derive(Clone)]
pub struct IdentifiedMessage {
    pub digest: String,
    pub message: Message,
}

#[derive(Serialize)]
pub struct MessageRecord {
    pub id: String,
    pub full_id: String,
    pub severity: &'static str,
    pub analyzer: String,
    pub config: String,
    pub location: serde_json::Value,
    pub relevance: Vec<usize>,
    pub text: String,
    pub dismissed: bool,
    pub resolved: bool,
    pub actions: usize,
}

pub fn universe(messages: Vec<Message>) -> Vec<IdentifiedMessage> {
    let mut unique = BTreeMap::new();
    for message in messages {
        unique.entry(digest(&message)).or_insert(IdentifiedMessage {
            digest: digest(&message),
            message,
        });
    }
    let mut values = unique.into_values().collect::<Vec<_>>();
    values.sort_by(|left, right| sort_key(left).cmp(&sort_key(right)));
    values
}

pub fn resolve<'a>(values: &'a [IdentifiedMessage], handle: &str) -> Result<&'a IdentifiedMessage> {
    let prefix = parse_handle(handle)?;
    let matches = values
        .iter()
        .filter(|value| value.digest.starts_with(prefix))
        .collect::<Vec<_>>();
    match matches.as_slice() {
    [] => bail!("unknown message ID {handle}"),
    [value] => Ok(value),
    _ => bail!("message ID is ambiguous"),
    }
}

pub fn parse_handle(handle: &str) -> Result<&str> {
    let prefix = handle
        .strip_prefix("m:")
        .ok_or_else(|| anyhow!("message ID must begin with m:"))?;
    if !(5..=128).contains(&prefix.len())
        || !prefix
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("message ID must contain 5 to 128 lowercase hexadecimal digits");
    }
    Ok(prefix)
}

pub fn records(
    values: &[IdentifiedMessage],
    dismissals: &[Dismissal],
    lifecore: &LifeCoreList,
    view: Context,
) -> Vec<MessageRecord> {
    values
        .iter()
        .map(|value| {
            let (severity, location, analyzer, config, text, relevance, actions) =
                parts(&value.message);
            MessageRecord {
                id: format!("m:{}", &value.digest[..16]),
                full_id: format!("m:{}", value.digest),
                severity,
                analyzer: analyzer.to_string(),
                config: config.to_string(),
                location: location_value(location),
                relevance: relevance.iter().map(|value| value.0).collect(),
                text: text.to_string(),
                dismissed: is_dismissed(dismissals, &value.digest),
                resolved: is_resolved(&value.message, lifecore, view),
                actions,
            }
        })
        .collect()
}

pub fn is_dismissed(dismissals: &[Dismissal], digest: &str) -> bool {
    dismissals
        .iter()
        .any(|value| value.version == MESSAGE_ID_VERSION && value.digest == digest)
}

pub fn is_resolved(message: &Message, lifecore: &LifeCoreList, view: Context) -> bool {
    let (_, location, _, _, _, relevance, _) = parts(message);
    let DiagnosticLocation::Entry(offending) = location else {
        return false;
    };
    let stale = |index: EntryIndex| {
        crate::locator::context_in_view(lifecore, index.0, view)
            .and_then(|context| lifecore.is_stale(context))
            == Some(true)
    };
    stale(offending.entry()) && relevance.iter().copied().all(stale)
}

pub fn parts(
    message: &Message,
) -> (
    &'static str,
    DiagnosticLocation,
    &str,
    &str,
    &str,
    Vec<EntryIndex>,
    usize,
) {
    match message {
    Message::Inconsistency(value) => (
        "Inconsistency",
        value.offending_event,
        &value.system_name,
        &value.system_config,
        &value.message,
        Vec::new(),
        0,
    ),
    Message::Warning(value) => (
        "Warning",
        value.offending_event,
        &value.system_name,
        &value.system_config,
        &value.message,
        value.relevance.clone(),
        0,
    ),
    Message::Suggestion(value) => (
        "Suggestion",
        value.offending_event,
        &value.system_name,
        &value.system_config,
        &value.message,
        value.relevance.clone(),
        value.action.len(),
    ),
    }
}

pub fn digest(message: &Message) -> String {
    let (severity, location, analyzer, config, text, mut relevance, _) = parts(message);
    relevance.sort();
    let mut output = Vec::new();
    bytes(&mut output, b"lifecore-message-id");
    output.extend_from_slice(&MESSAGE_ID_VERSION.to_be_bytes());
    bytes(&mut output, severity.as_bytes());
    bytes(&mut output, analyzer.as_bytes());
    bytes(&mut output, config.as_bytes());
    location_bytes(&mut output, location);
    bytes(&mut output, text.as_bytes());
    output.extend_from_slice(&(relevance.len() as u64).to_be_bytes());
    for value in relevance {
        output.extend_from_slice(&(value.0 as u64).to_be_bytes());
    }
    if let Message::Suggestion(value) = message {
        output.extend_from_slice(&(value.action.len() as u64).to_be_bytes());
        for action in &value.action {
            action_bytes(&mut output, action);
        }
    } else {
        output.extend_from_slice(&0u64.to_be_bytes());
    }
    format!("{:x}", Sha3_512::digest(output))
}

fn sort_key(value: &IdentifiedMessage) -> (usize, usize, u8, &str, &str, &str, &str) {
    let (severity, location, analyzer, config, text, _, _) = parts(&value.message);
    let (end, entry) = location_key(location);
    let rank = match severity {
    "Inconsistency" => 0,
    "Warning" => 1,
    _ => 2,
    };
    (end, entry, rank, analyzer, config, text, &value.digest)
}

fn location_key(location: DiagnosticLocation) -> (usize, usize) {
    match location {
    DiagnosticLocation::Entry(value) => (context_end(value.window()), value.entry().0),
    DiagnosticLocation::Context(value) => (context_end(value), usize::MAX),
    }
}

fn location_value(location: DiagnosticLocation) -> serde_json::Value {
    match location {
    DiagnosticLocation::Context(value) => {
        serde_json::json!({"context_end": context_end(value)})
    }
    DiagnosticLocation::Entry(value) => serde_json::json!({
        "context_end": context_end(value.window()),
        "entry": value.entry().0,
    }),
    }
}

fn location_bytes(output: &mut Vec<u8>, location: DiagnosticLocation) {
    match location {
    DiagnosticLocation::Context(value) => {
        output.push(1);
        output.extend_from_slice(&(context_end(value) as u64).to_be_bytes());
    }
    DiagnosticLocation::Entry(value) => {
        output.push(2);
        output.extend_from_slice(&(context_end(value.window()) as u64).to_be_bytes());
        output.extend_from_slice(&(value.entry().0 as u64).to_be_bytes());
    }
    }
}

fn context_end(value: Context) -> usize {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.get("end").and_then(serde_json::Value::as_u64))
        .unwrap_or(usize::MAX as u64) as usize
}

fn action_bytes(output: &mut Vec<u8>, action: &SuggestedAction) {
    match action {
    SuggestedAction::AddNode {
        ty_name,
        payload,
        parent,
    } => {
        output.push(1);
        bytes(output, ty_name.as_bytes());
        body_bytes(output, payload);
        match parent {
        Some(parent) => {
            output.push(1);
            output.extend_from_slice(&(parent.0 as u64).to_be_bytes());
        }
        None => output.push(0),
        }
    }
    SuggestedAction::AddEdge { from, ty_name, to } => {
        output.push(2);
        output.extend_from_slice(&(from.0 as u64).to_be_bytes());
        bytes(output, ty_name.as_bytes());
        output.extend_from_slice(&(to.0 as u64).to_be_bytes());
    }
    }
}

fn body_bytes(output: &mut Vec<u8>, body: &Body) {
    output.extend_from_slice(&(body.len() as u64).to_be_bytes());
    for (name, value) in body.iter() {
        bytes(output, name.as_bytes());
        payload_bytes(output, value);
    }
}

fn payload_bytes(output: &mut Vec<u8>, payload: &Payload) {
    match payload {
    Payload::PreciseTime(value) => {
        output.push(1);
        bytes(output, value.to_string().as_bytes());
    }
    Payload::Text(value) => {
        output.push(2);
        bytes(output, value.as_bytes());
    }
    Payload::Number(value) => {
        output.push(3);
        output.extend_from_slice(&value.to_be_bytes());
    }
    Payload::Sequence(values) => {
        output.push(4);
        output.extend_from_slice(&(values.len() as u64).to_be_bytes());
        values.iter().for_each(|value| payload_bytes(output, value));
    }
    Payload::OrderedSet(values) => {
        output.push(5);
        output.extend_from_slice(&(values.len() as u64).to_be_bytes());
        values.iter().for_each(|value| payload_bytes(output, value));
    }
    }
}

fn bytes(output: &mut Vec<u8>, value: &[u8]) {
    output.extend_from_slice(&(value.len() as u64).to_be_bytes());
    output.extend_from_slice(value);
}
