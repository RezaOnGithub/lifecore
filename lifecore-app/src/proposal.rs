use lifecore::{
    Body, EntryId, View,
    analysis::FindingKey,
    transaction::{Change, NewEdge, NewNode, Reference, RevisionIntent},
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use std::{collections::HashMap, fmt, path::Path, str::FromStr};
use uuid::Uuid;

//A A small bounded version field that accepts exactly one value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FormatVersion<const V: u32>;

impl<const V: u32> Serialize for FormatVersion<V> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(V)
    }
}

impl<'de, const V: u32> Deserialize<'de> for FormatVersion<V> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let version = u32::deserialize(deserializer)?;
        if version != V {
            return Err(D::Error::custom(format!(
                "unsupported format_version {version}; expected {V}"
            )));
        }
        Ok(Self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub format_version: FormatVersion<1>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub plan_id: Option<PlanId>,
    #[serde(
        default,
        deserialize_with = "present",
        skip_serializing_if = "Option::is_none"
    )]
    pub acknowledges: Option<FindingKey>,
    pub entries: Vec<ProposalEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", try_from = "RawEntry")]
pub enum ProposalEntry {
    Node {
        name: String,
        #[serde(rename = "type")]
        ty_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        predecessor: Option<ProposalReference>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        intent: Option<RevisionIntent>,
        payload: Body,
    },
    Edge {
        name: String,
        #[serde(rename = "type")]
        ty_name: String,
        from: ProposalReference,
        to: ProposalReference,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawKind {
    Node,
    Edge,
}

//A Entries decode through this flat shape rather than a buffered tagged enum,
//A so errors inside a payload keep their exact source location.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEntry {
    name: String,
    kind: RawKind,
    #[serde(rename = "type")]
    ty_name: String,
    #[serde(default, deserialize_with = "present")]
    predecessor: Option<ProposalReference>,
    #[serde(default, deserialize_with = "present")]
    intent: Option<RevisionIntent>,
    #[serde(default, deserialize_with = "present")]
    payload: Option<Body>,
    #[serde(default, deserialize_with = "present")]
    from: Option<ProposalReference>,
    #[serde(default, deserialize_with = "present")]
    to: Option<ProposalReference>,
}

//A Optional control fields are either omitted or valid; JSON null is not an
//A alternative spelling of absence.
fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

impl TryFrom<RawEntry> for ProposalEntry {
    type Error = String;
    fn try_from(raw: RawEntry) -> Result<Self, String> {
        let unexpected = |kind: &str, fields: &[(&str, bool)]| {
            fields
                .iter()
                .find(|(_, present)| *present)
                .map(|(field, _)| format!("{kind} entry {:?} cannot have `{field}`", raw.name))
        };
        match raw.kind {
        RawKind::Node => {
            if let Some(error) = unexpected(
                "node",
                &[("from", raw.from.is_some()), ("to", raw.to.is_some())],
            ) {
                return Err(error);
            }
            Ok(Self::Node {
                payload: raw
                    .payload
                    .ok_or_else(|| format!("node entry {:?} requires `payload`", raw.name))?,
                name: raw.name,
                ty_name: raw.ty_name,
                predecessor: raw.predecessor,
                intent: raw.intent,
            })
        }
        RawKind::Edge => {
            if let Some(error) = unexpected(
                "edge",
                &[
                    ("payload", raw.payload.is_some()),
                    ("predecessor", raw.predecessor.is_some()),
                    ("intent", raw.intent.is_some()),
                ],
            ) {
                return Err(error);
            }
            let (Some(from), Some(to)) = (raw.from, raw.to) else {
                return Err(format!(
                    "edge entry {:?} requires `from` and `to`",
                    raw.name
                ));
            };
            Ok(Self::Edge {
                name: raw.name,
                ty_name: raw.ty_name,
                from,
                to,
            })
        }
        }
    }
}

impl ProposalEntry {
    pub fn name(&self) -> &str {
        match self {
        Self::Node { name, .. } | Self::Edge { name, .. } => name,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalReference {
    Existing(EntryId),
    Added(String),
}

//A `lc1:<database>:<graph revision>:<proposal>`, each a lowercase hyphenated UUID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlanId {
    pub database: Uuid,
    pub graph_revision: Uuid,
    pub proposal: Uuid,
}

impl fmt::Display for PlanId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "lc1:{}:{}:{}",
            self.database.hyphenated(),
            self.graph_revision.hyphenated(),
            self.proposal.hyphenated()
        )
    }
}

impl FromStr for PlanId {
    type Err = String;
    fn from_str(text: &str) -> Result<Self, String> {
        let malformed = || format!("malformed plan ID {text:?}");
        let [version, database, graph_revision, proposal] = text.split(':').collect::<Vec<_>>()[..]
        else {
            return Err(malformed());
        };
        if version != "lc1" {
            return Err(format!("unsupported plan ID version {version:?}"));
        }
        //A Only the canonical spelling is accepted, so a token has one form.
        let uuid = |part: &str| {
            Uuid::try_parse(part)
                .ok()
                .filter(|id| id.hyphenated().to_string() == part)
                .ok_or_else(malformed)
        };
        Ok(Self {
            database: uuid(database)?,
            graph_revision: uuid(graph_revision)?,
            proposal: uuid(proposal)?,
        })
    }
}

impl Serialize for PlanId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for PlanId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(D::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Json,
    Toml,
}

impl Format {
    pub fn from_path(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()? {
        "json" => Some(Self::Json),
        "toml" => Some(Self::Toml),
        _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{message}")]
pub struct DecodeError {
    pub format: Format,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
}

pub fn decode(text: &str, format: Format) -> Result<Proposal, DecodeError> {
    match format {
    Format::Json => serde_json::from_str(text).map_err(|error| DecodeError {
        format,
        message: error.to_string(),
        line: Some(error.line()),
        column: Some(error.column()),
    }),
    Format::Toml => toml::from_str(text).map_err(|error| {
        //A TOML reports byte spans; convert the start to a one-based line/column.
        let (line, column) = error
            .span()
            .map(|span| {
                let before = &text[..span.start.min(text.len())];
                let line = before.matches('\n').count() + 1;
                let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
                (Some(line), Some(column))
            })
            .unwrap_or_default();
        //A HACK: the toml crate presents native datetimes as a map with a
        //A private marker key, which the payload decoder reports as an unknown
        //A field. Name the actual mistake instead.
        let message = if error.message().contains("$__toml_private_datetime") {
            "native TOML datetimes are not payload values; write { kind = \"timestamp\", value = \"...\" }".into()
        } else {
            error.message().to_string()
        };
        DecodeError {
            format,
            message,
            line,
            column,
        }
    }),
    }
}

pub fn encode(proposal: &Proposal, format: Format) -> Result<String, String> {
    match format {
    Format::Json => serde_json::to_string_pretty(proposal)
        .map(|text| text + "\n")
        .map_err(|error| error.to_string()),
    Format::Toml => toml::to_string_pretty(proposal).map_err(|error| error.to_string()),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalLocation {
    pub operation: usize,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum LoweringError {
    #[error("operation name {name:?} must match [A-Za-z_][A-Za-z0-9_-]*")]
    InvalidName { name: String },
    #[error("operation name {name:?} is already used by an earlier entry")]
    DuplicateName { name: String },
    #[error("{name:?} does not name an earlier node entry of this proposal")]
    UnknownOperation { name: String },
    #[error("{name:?} names an edge; predecessors and endpoints must be nodes")]
    EdgeReference { name: String },
    #[error("entry {id:?} is not a node in the selected starting state")]
    MissingEntry { id: EntryId },
}

fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

impl Proposal {
    //A Lowering resolves names to operation indexes and checks that exact
    //A references exist in the base. All graph rules, including intent, are
    //A left to `transaction::prepare` at each prefix.
    pub fn lower(&self, base: View<'_>) -> Result<Vec<Change>, (ProposalLocation, LoweringError)> {
        let mut operations = HashMap::<&str, (usize, bool)>::new();
        let mut changes = Vec::with_capacity(self.entries.len());
        for (operation, entry) in self.entries.iter().enumerate() {
            let at = |field: Option<&str>| ProposalLocation {
                operation,
                name: entry.name().into(),
                field: field.map(Into::into),
            };
            let name = entry.name();
            if !valid_name(name) {
                return Err((
                    at(Some("name")),
                    LoweringError::InvalidName { name: name.into() },
                ));
            }
            if operations.contains_key(name) {
                return Err((
                    at(Some("name")),
                    LoweringError::DuplicateName { name: name.into() },
                ));
            }
            let resolve = |reference: &ProposalReference, field: &str| match reference {
            ProposalReference::Existing(id) => base
                .node(*id)
                .map(|_| Reference::Existing(*id))
                .ok_or((at(Some(field)), LoweringError::MissingEntry { id: *id })),
            ProposalReference::Added(target) => {
                match operations.get(target.as_str()) {
                Some((index, true)) => Ok(Reference::Added(*index)),
                Some((_, false)) => Err((
                    at(Some(field)),
                    LoweringError::EdgeReference {
                        name: target.clone(),
                    },
                )),
                None => Err((
                    at(Some(field)),
                    LoweringError::UnknownOperation {
                        name: target.clone(),
                    },
                )),
                }
            }
            };
            changes.push(match entry {
            ProposalEntry::Node {
                ty_name,
                predecessor,
                intent,
                payload,
                ..
            } => Change::AddNode(NewNode {
                ty_name: ty_name.clone(),
                body: payload.clone(),
                parent: predecessor
                    .as_ref()
                    .map(|reference| resolve(reference, "predecessor"))
                    .transpose()?,
                intent: *intent,
            }),
            ProposalEntry::Edge {
                ty_name, from, to, ..
            } => Change::AddEdge(NewEdge {
                from: resolve(from, "from")?,
                ty_name: ty_name.clone(),
                to: resolve(to, "to")?,
            }),
            });
            operations.insert(
                name,
                (operation, matches!(entry, ProposalEntry::Node { .. })),
            );
        }
        Ok(changes)
    }
}
