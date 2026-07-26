use std::{cmp::Ordering, collections::BTreeMap};

use lifecore::{
    Body, Context, DiagnosticLocation, EntryIndex, LifeCore, LifeCoreList, Payload,
    analysis::{Message, SuggestedAction},
};
use serde::Serialize;
use sha3::{Digest, Sha3_512};

use crate::{
    analysis::AnalysisReport,
    error::AppError,
    state::{DismissalRecord, MESSAGE_ID_VERSION},
};

pub const MESSAGE_DISPLAY_DIGITS: usize = 16;
const MESSAGE_PREFIX: &str = "m:";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Inconsistency,
    Warning,
    Suggestion,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
        Self::Inconsistency => "Inconsistency",
        Self::Warning => "Warning",
        Self::Suggestion => "Suggestion",
        }
    }
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MessageLocation {
    Context { end: u64 },
    Entry { context_end: u64, entry_index: u64 },
}

impl MessageLocation {
    pub fn display(&self) -> String {
        match self {
        Self::Context { end } => format!("context 0..{end}"),
        Self::Entry {
            context_end,
            entry_index,
        } => format!("entry {entry_index} in 0..{context_end}"),
        }
    }

    fn offending_index(&self) -> Option<u64> {
        match self {
        Self::Context { .. } => None,
        Self::Entry { entry_index, .. } => Some(*entry_index),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActionRecord {
    AddNode {
        r#type: String,
        body: Body,
        parent: Option<u64>,
    },
    AddEdge {
        from: u64,
        r#type: String,
        to: u64,
    },
}

impl ActionRecord {
    pub fn summary(&self) -> String {
        match self {
        Self::AddNode { r#type, .. } => format!("add node {}", r#type),
        Self::AddEdge {
            from, r#type, to, ..
        } => format!("connect {from} -[{}]-> {to}", r#type),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MessageId {
    digest: [u8; 64],
}

impl MessageId {
    pub fn full_digest(&self) -> String {
        hex::encode(self.digest)
    }

    pub fn display(&self) -> String {
        format!(
            "{MESSAGE_PREFIX}{}",
            &self.full_digest()[..MESSAGE_DISPLAY_DIGITS]
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MessageRecord {
    pub id_version: u32,
    pub id: String,
    pub display_id: String,
    pub severity: Severity,
    pub analyzer: String,
    pub analyzer_config: String,
    pub location: MessageLocation,
    pub relevance: Vec<u64>,
    pub text: String,
    pub actions: Vec<ActionRecord>,
    pub dismissed: bool,
    pub resolved: bool,
    pub visible: bool,
}

impl MessageRecord {
    pub fn action_summary(&self) -> String {
        self.actions
            .iter()
            .map(ActionRecord::summary)
            .collect::<Vec<_>>()
            .join("; ")
    }
}

#[derive(Clone, Debug)]
pub struct MessageUniverse {
    records: Vec<MessageRecord>,
    entry_count: usize,
}

impl MessageUniverse {
    pub fn from_analysis(
        report: &AnalysisReport,
        lifecore: &LifeCoreList,
        dismissals: &[DismissalRecord],
    ) -> Result<Self, AppError> {
        let dismissal_set = dismissals
            .iter()
            .map(|record| record.digest.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        let mut deduplicated = BTreeMap::<String, MessageRecord>::new();

        for message in &report.messages {
            let canonical = CanonicalMessage::from_message(message, report.entry_count)?;
            let message_id = canonical.message_id();
            let full_digest = message_id.full_digest();
            let severity = canonical.severity;
            let dismissed =
                severity != Severity::Inconsistency && dismissal_set.contains(full_digest.as_str());
            let resolved = severity != Severity::Inconsistency
                && canonical.is_resolved(lifecore, report.entry_count);
            let visible = severity == Severity::Inconsistency || (!dismissed && !resolved);

            let record = canonical.into_record(message_id, dismissed, resolved, visible);

            match deduplicated.entry(full_digest) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(record);
            }
            std::collections::btree_map::Entry::Occupied(entry) => {
                if entry.get() != &record {
                    return Err(AppError::analysis(
                        "distinct analyzer messages produced the same full message digest",
                    ));
                }
            }
            }
        }

        let mut records = deduplicated.into_values().collect::<Vec<_>>();
        records.sort_by(compare_records);

        Ok(Self {
            records,
            entry_count: report.entry_count,
        })
    }

    pub fn records(&self) -> &[MessageRecord] {
        &self.records
    }

    pub fn visible(&self) -> Vec<MessageRecord> {
        self.records
            .iter()
            .filter(|record| record.visible)
            .cloned()
            .collect()
    }

    pub fn resolve(&self, prefix: &str) -> Result<MessageRecord, AppError> {
        let digest_prefix = parse_message_prefix(prefix)?;
        let matches = self
            .records
            .iter()
            .filter(|record| record.id.starts_with(digest_prefix))
            .collect::<Vec<_>>();

        match matches.as_slice() {
        [] => Err(AppError::unknown_message_id(prefix)),
        [record] => Ok((*record).clone()),
        _ => Err(AppError::ambiguous_message_id(prefix)),
        }
    }

    pub fn entry_count(&self) -> usize {
        self.entry_count
    }
}

fn compare_records(left: &MessageRecord, right: &MessageRecord) -> Ordering {
    location_sort_key(&left.location)
        .cmp(&location_sort_key(&right.location))
        .then_with(|| left.severity.cmp(&right.severity))
        .then_with(|| left.analyzer.cmp(&right.analyzer))
        .then_with(|| left.analyzer_config.cmp(&right.analyzer_config))
        .then_with(|| left.text.cmp(&right.text))
        .then_with(|| left.id.cmp(&right.id))
}

fn location_sort_key(location: &MessageLocation) -> (u64, Option<u64>) {
    match location {
    MessageLocation::Context { end } => (*end, None),
    MessageLocation::Entry {
        context_end,
        entry_index,
    } => (*context_end, Some(*entry_index)),
    }
}

fn parse_message_prefix(value: &str) -> Result<&str, AppError> {
    let Some(digest) = value.strip_prefix(MESSAGE_PREFIX) else {
        return Err(AppError::invalid_message_id(
            "expected m: followed by 5 to 128 lowercase hexadecimal digits",
        ));
    };

    if !(5..=128).contains(&digest.len())
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(AppError::invalid_message_id(
            "expected m: followed by 5 to 128 lowercase hexadecimal digits",
        ));
    }

    Ok(digest)
}

#[derive(Clone)]
struct CanonicalMessage {
    severity: Severity,
    analyzer: String,
    analyzer_config: String,
    location: MessageLocation,
    relevance: Vec<u64>,
    text: String,
    actions: Vec<ActionRecord>,
}

impl CanonicalMessage {
    fn from_message(message: &Message, entry_count: usize) -> Result<Self, AppError> {
        let (severity, analyzer, analyzer_config, location, relevance, text, actions) =
            match message {
            Message::Inconsistency(value) => (
                Severity::Inconsistency,
                value.system_name.clone(),
                value.system_config.clone(),
                value.offending_event,
                Vec::new(),
                value.message.clone(),
                Vec::new(),
            ),
            Message::Warning(value) => (
                Severity::Warning,
                value.system_name.clone(),
                value.system_config.clone(),
                value.offending_event,
                value.relevance.clone(),
                value.message.clone(),
                Vec::new(),
            ),
            Message::Suggestion(value) => (
                Severity::Suggestion,
                value.system_name.clone(),
                value.system_config.clone(),
                value.offending_event,
                value.relevance.clone(),
                value.message.clone(),
                value.action.clone(),
            ),
            };

        let location = normalize_location(location, entry_count)?;
        let mut relevance = relevance
            .into_iter()
            .map(|entry| checked_index(entry, entry_count, "message relevance"))
            .collect::<Result<Vec<_>, _>>()?;
        relevance.sort_unstable();

        let actions = actions
            .into_iter()
            .map(|action| normalize_action(action, entry_count))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            severity,
            analyzer,
            analyzer_config,
            location,
            relevance,
            text,
            actions,
        })
    }

    fn message_id(&self) -> MessageId {
        let mut encoded = Vec::new();
        encode_bytes(&mut encoded, b"lifecore-message-id");
        encode_u64(&mut encoded, u64::from(MESSAGE_ID_VERSION));
        encode_u8(
            &mut encoded,
            match self.severity {
            Severity::Inconsistency => 0,
            Severity::Warning => 1,
            Severity::Suggestion => 2,
            },
        );
        encode_string(&mut encoded, &self.analyzer);
        encode_string(&mut encoded, &self.analyzer_config);
        encode_location(&mut encoded, &self.location);
        encode_string(&mut encoded, &self.text);
        encode_u64(&mut encoded, self.relevance.len() as u64);
        for entry in &self.relevance {
            encode_u64(&mut encoded, *entry);
        }
        encode_u64(&mut encoded, self.actions.len() as u64);
        for action in &self.actions {
            encode_action(&mut encoded, action);
        }

        MessageId {
            digest: Sha3_512::digest(encoded).into(),
        }
    }

    fn is_resolved(&self, lifecore: &LifeCoreList, entry_count: usize) -> bool {
        let Some(offending) = self.location.offending_index() else {
            return false;
        };

        is_stale(lifecore, offending, entry_count)
            && self
                .relevance
                .iter()
                .all(|entry| is_stale(lifecore, *entry, entry_count))
    }

    fn into_record(
        self,
        id: MessageId,
        dismissed: bool,
        resolved: bool,
        visible: bool,
    ) -> MessageRecord {
        MessageRecord {
            id_version: MESSAGE_ID_VERSION,
            id: id.full_digest(),
            display_id: id.display(),
            severity: self.severity,
            analyzer: self.analyzer,
            analyzer_config: self.analyzer_config,
            location: self.location,
            relevance: self.relevance,
            text: self.text,
            actions: self.actions,
            dismissed,
            resolved,
            visible,
        }
    }
}

fn normalize_location(
    location: DiagnosticLocation,
    entry_count: usize,
) -> Result<MessageLocation, AppError> {
    match location {
    DiagnosticLocation::Context(context) => Ok(MessageLocation::Context {
        end: checked_context_end(context, entry_count)?,
    }),
    DiagnosticLocation::Entry(entry) => {
        let entry_index = checked_index(entry.entry(), entry_count, "message location")?;
        let context_end = checked_context_end(entry.window(), entry_count)?;

        if entry_index >= context_end {
            return Err(AppError::analysis(
                "analyzer emitted an entry location outside its context",
            ));
        }

        Ok(MessageLocation::Entry {
            context_end,
            entry_index,
        })
    }
    }
}

fn checked_context_end(context: Context, entry_count: usize) -> Result<u64, AppError> {
    let end = if context == Context::universe() {
        entry_count
    } else {
        context.last().map_or(0, |entry| entry.entry().0 + 1)
    };

    if end > entry_count {
        return Err(AppError::analysis(
            "analyzer emitted a diagnostic context beyond graph history",
        ));
    }

    u64::try_from(end).map_err(|_| AppError::analysis("graph context does not fit in message ID"))
}

fn checked_index(entry: EntryIndex, entry_count: usize, field: &str) -> Result<u64, AppError> {
    if entry.0 >= entry_count {
        return Err(AppError::analysis(format!(
            "analyzer emitted an out-of-range {field} index {}",
            entry.0
        )));
    }

    u64::try_from(entry.0).map_err(|_| AppError::analysis("entry index does not fit in message ID"))
}

fn normalize_action(action: SuggestedAction, entry_count: usize) -> Result<ActionRecord, AppError> {
    match action {
    SuggestedAction::AddNode {
        ty_name,
        payload,
        parent,
    } => Ok(ActionRecord::AddNode {
        r#type: ty_name,
        body: payload,
        parent: parent
            .map(|entry| checked_index(entry, entry_count, "suggested parent"))
            .transpose()?,
    }),
    SuggestedAction::AddEdge { from, ty_name, to } => Ok(ActionRecord::AddEdge {
        from: checked_index(from, entry_count, "suggested edge endpoint")?,
        r#type: ty_name,
        to: checked_index(to, entry_count, "suggested edge endpoint")?,
    }),
    }
}

fn is_stale(lifecore: &LifeCoreList, entry: u64, entry_count: usize) -> bool {
    let Ok(entry) = usize::try_from(entry) else {
        return false;
    };

    if entry >= entry_count {
        return false;
    }

    lifecore
        .is_stale(EntryIndex(entry).absolute())
        .unwrap_or(false)
}

fn encode_u8(output: &mut Vec<u8>, value: u8) {
    output.push(value);
}

fn encode_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn encode_bytes(output: &mut Vec<u8>, value: &[u8]) {
    encode_u64(output, value.len() as u64);
    output.extend_from_slice(value);
}

fn encode_string(output: &mut Vec<u8>, value: &str) {
    encode_bytes(output, value.as_bytes());
}

fn encode_location(output: &mut Vec<u8>, location: &MessageLocation) {
    match location {
    MessageLocation::Context { end } => {
        encode_u8(output, 0);
        encode_u64(output, *end);
    }
    MessageLocation::Entry {
        context_end,
        entry_index,
    } => {
        encode_u8(output, 1);
        encode_u64(output, *context_end);
        encode_u64(output, *entry_index);
    }
    }
}

fn encode_action(output: &mut Vec<u8>, action: &ActionRecord) {
    match action {
    ActionRecord::AddNode {
        r#type,
        body,
        parent,
    } => {
        encode_u8(output, 0);
        encode_string(output, r#type);
        encode_body(output, body);
        match parent {
        Some(parent) => {
            encode_u8(output, 1);
            encode_u64(output, *parent);
        }
        None => encode_u8(output, 0),
        }
    }
    ActionRecord::AddEdge {
        from, r#type, to, ..
    } => {
        encode_u8(output, 1);
        encode_u64(output, *from);
        encode_string(output, r#type);
        encode_u64(output, *to);
    }
    }
}

fn encode_body(output: &mut Vec<u8>, body: &Body) {
    encode_u64(output, body.iter().count() as u64);
    for (name, payload) in body.iter() {
        encode_string(output, name);
        encode_payload(output, payload);
    }
}

fn encode_payload(output: &mut Vec<u8>, payload: &Payload) {
    match payload {
    Payload::PreciseTime(value) => {
        encode_u8(output, 0);
        encode_string(output, &value.to_string());
    }
    Payload::Text(value) => {
        encode_u8(output, 1);
        encode_string(output, value);
    }
    Payload::Number(value) => {
        encode_u8(output, 2);
        output.extend_from_slice(&value.to_be_bytes());
    }
    Payload::Sequence(values) => {
        encode_u8(output, 3);
        encode_u64(output, values.len() as u64);
        for value in values {
            encode_payload(output, value);
        }
    }
    Payload::OrderedSet(values) => {
        encode_u8(output, 4);
        encode_u64(output, values.len() as u64);
        for value in values {
            encode_payload(output, value);
        }
    }
    }
}
