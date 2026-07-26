use std::{cmp::Ordering, collections::BTreeMap};

use lifecore::{
    Body, Context, DiagnosticLocation, EntryContext, EntryIndex, LifeCore, LifeCoreList,
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
        encode_lower_hex(&self.digest)
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
            let canonical = CanonicalMessage::from_message(message)?;
            let message_id = canonical.message_id()?;
            let full_digest = message_id.full_digest();
            let severity = canonical.severity;
            let dismissed =
                severity != Severity::Inconsistency && dismissal_set.contains(full_digest.as_str());
            let resolved =
                severity != Severity::Inconsistency && canonical.is_resolved(lifecore)?;
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

        Ok(Self { records })
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
    offending_entry: Option<EntryContext>,
    relevance: Vec<u64>,
    text: String,
    actions: Vec<ActionRecord>,
}

//A This DTO is the complete v1 message-identity wire contract. Keeping it
//A separate from presentation records prevents display-only fields from
//A changing persisted diagnostic identities.
#[derive(Serialize)]
struct MessageIdentityV1<'a> {
    domain: &'static str,
    version: u32,
    severity: Severity,
    analyzer: &'a str,
    analyzer_config: &'a str,
    location: MessageIdentityLocationV1,
    text: &'a str,
    relevance: &'a [u64],
    actions: Vec<MessageIdentityActionV1<'a>>,
}

#[derive(Serialize)]
enum MessageIdentityLocationV1 {
    Context { end: u64 },
    Entry { context_end: u64, entry_index: u64 },
}

#[derive(Serialize)]
enum MessageIdentityActionV1<'a> {
    AddNode {
        r#type: &'a str,
        body: &'a Body,
        parent: Option<u64>,
    },
    AddEdge {
        from: u64,
        r#type: &'a str,
        to: u64,
    },
}

impl CanonicalMessage {
    fn from_message(message: &Message) -> Result<Self, AppError> {
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

        let offending_entry = match location {
        DiagnosticLocation::Context(_) => None,
        DiagnosticLocation::Entry(entry) => Some(entry),
        };
        let location = normalize_location(location)?;
        let mut relevance = relevance
            .into_iter()
            .map(|entry| normalize_index(entry, "message relevance"))
            .collect::<Result<Vec<_>, _>>()?;
        //A Relevance is a set for identity purposes, while SuggestedActions
        //A remain ordered because changing their order can change the repair.
        relevance.sort_unstable();

        let actions = actions
            .into_iter()
            .map(normalize_action)
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            severity,
            analyzer,
            analyzer_config,
            location,
            offending_entry,
            relevance,
            text,
            actions,
        })
    }

    fn identity_bytes(&self) -> Result<Vec<u8>, AppError> {
        let identity = MessageIdentityV1 {
            domain: "lifecore-message-id",
            version: MESSAGE_ID_VERSION,
            severity: self.severity,
            analyzer: &self.analyzer,
            analyzer_config: &self.analyzer_config,
            location: MessageIdentityLocationV1::from(&self.location),
            text: &self.text,
            relevance: &self.relevance,
            actions: self
                .actions
                .iter()
                .map(MessageIdentityActionV1::from)
                .collect(),
        };

        postcard::to_allocvec(&identity)
            .map_err(|error| AppError::analysis(format!("message ID encoding failed: {error}")))
    }

    fn message_id(&self) -> Result<MessageId, AppError> {
        Ok(MessageId {
            digest: Sha3_512::digest(self.identity_bytes()?).into(),
        })
    }

    fn is_resolved(&self, lifecore: &LifeCoreList) -> Result<bool, AppError> {
        let Some(offending) = self.offending_entry else {
            return Ok(false);
        };

        if lifecore.is_stale(offending) != Some(true) {
            return Ok(false);
        }

        let MessageLocation::Entry { context_end, .. } = self.location else {
            unreachable!("pointed diagnostics normalize to entry locations");
        };

        for entry in &self.relevance {
            if *entry >= context_end {
                return Err(AppError::analysis(
                    "analyzer emitted message relevance outside the diagnostic context",
                ));
            }

            let entry = usize::try_from(*entry)
                .map_err(|_| AppError::analysis("message relevance does not fit this platform"))?;
            let pointed = offending.window().look_at(EntryIndex(entry).absolute());

            if lifecore.is_stale(pointed) != Some(true) {
                return Ok(false);
            }
        }

        Ok(true)
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

impl From<&MessageLocation> for MessageIdentityLocationV1 {
    fn from(location: &MessageLocation) -> Self {
        match location {
        MessageLocation::Context { end } => Self::Context { end: *end },
        MessageLocation::Entry {
            context_end,
            entry_index,
        } => Self::Entry {
            context_end: *context_end,
            entry_index: *entry_index,
        },
        }
    }
}

impl<'a> From<&'a ActionRecord> for MessageIdentityActionV1<'a> {
    fn from(action: &'a ActionRecord) -> Self {
        match action {
        ActionRecord::AddNode {
            r#type,
            body,
            parent,
        } => Self::AddNode {
            r#type,
            body,
            parent: *parent,
        },
        ActionRecord::AddEdge { from, r#type, to } => Self::AddEdge {
            from: *from,
            r#type,
            to: *to,
        },
        }
    }
}

fn normalize_location(location: DiagnosticLocation) -> Result<MessageLocation, AppError> {
    match location {
    DiagnosticLocation::Context(context) => Ok(MessageLocation::Context {
        end: bounded_context_end(context)?,
    }),
    DiagnosticLocation::Entry(entry) => Ok(MessageLocation::Entry {
        context_end: bounded_context_end(entry.window())?,
        entry_index: normalize_index(entry.entry(), "message location")?,
    }),
    }
}

fn bounded_context_end(context: Context) -> Result<u64, AppError> {
    if context == Context::universe() {
        return Err(AppError::analysis(
            "analyzer emitted an unbounded diagnostic context",
        ));
    }

    let end = context.last().map_or(0, |entry| entry.entry().0 + 1);
    u64::try_from(end).map_err(|_| AppError::analysis("graph context does not fit in message ID"))
}

fn normalize_index(entry: EntryIndex, field: &str) -> Result<u64, AppError> {
    u64::try_from(entry.0)
        .map_err(|_| AppError::analysis(format!("{field} index does not fit in message ID")))
}

fn normalize_action(action: SuggestedAction) -> Result<ActionRecord, AppError> {
    match action {
    SuggestedAction::AddNode {
        ty_name,
        payload,
        parent,
    } => Ok(ActionRecord::AddNode {
        r#type: ty_name,
        body: payload,
        parent: parent
            .map(|entry| normalize_index(entry, "suggested parent"))
            .transpose()?,
    }),
    SuggestedAction::AddEdge { from, ty_name, to } => Ok(ActionRecord::AddEdge {
        from: normalize_index(from, "suggested edge endpoint")?,
        r#type: ty_name,
        to: normalize_index(to, "suggested edge endpoint")?,
    }),
    }
}

pub(crate) fn encode_lower_hex(digest: &[u8; 64]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";

    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }

    encoded
}

#[cfg(test)]
pub(crate) fn message_identity_bytes(message: &Message) -> Result<Vec<u8>, AppError> {
    CanonicalMessage::from_message(message)?.identity_bytes()
}
