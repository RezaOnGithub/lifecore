use crate::{Body, EntryId, Payload, PayloadKind, View};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FieldStatus {
    Mandatory,
    Encouraged,
    Deprecated,
    Optional,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ValueType {
    Text,
    Integer,
    Boolean,
    Timestamp,
    List(Box<ValueType>),
    Record(BTreeMap<String, FieldDecl>),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FieldDecl {
    pub ty: ValueType,
    pub status: FieldStatus,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Type {
    pub name: String,
    pub fields: BTreeMap<String, FieldDecl>,
    pub outgoing: BTreeSet<String>,
    pub incoming: BTreeSet<String>,
    pub successors: BTreeSet<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PathPart {
    Field(String),
    Index(usize),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum FieldIssueKind {
    MissingMandatory,
    MissingEncouraged,
    Deprecated,
    UnexpectedField,
    TypeMismatch {
        expected: PayloadKind,
        actual: PayloadKind,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FieldIssue {
    pub path: Vec<PathPart>,
    pub kind: FieldIssueKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, thiserror::Error)]
#[error("invalid schema at {path:?}: {kind:?}")]
pub struct SchemaError {
    pub path: Vec<PathPart>,
    pub kind: SchemaErrorKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SchemaErrorKind {
    MissingField,
    UnknownField,
    WrongShape {
        expected: PayloadKind,
        actual: PayloadKind,
    },
    MissingPresence,
    UnknownPresence(String),
    MissingShape,
    UnknownShape(String),
    MissingRecordFields,
    TrailingElements,
    TooDeep,
    DuplicatePermission(String),
}

impl SchemaError {
    fn new(kind: SchemaErrorKind) -> Self {
        Self { path: vec![], kind }
    }
    fn field(mut self, name: &str) -> Self {
        self.path.insert(0, PathPart::Field(name.into()));
        self
    }
    fn index(mut self, index: usize) -> Self {
        self.path.insert(0, PathPart::Index(index));
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeDefinition {
    Builtin,
    Declared { id: EntryId, schema: Type },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum ResolutionError {
    #[error("type {name:?} has multiple active declarations")]
    Ambiguous { name: String, entries: Vec<EntryId> },
    #[error("malformed declaration {id:?}: {error}")]
    Malformed { id: EntryId, error: SchemaError },
}

impl View<'_> {
    pub fn resolve_type(self, name: &str) -> Result<Option<TypeDefinition>, ResolutionError> {
        if name == Type::TYPE_NAME {
            return Ok(Some(TypeDefinition::Builtin));
        }
        let mut found = vec![];
        for node in self.fresh_nodes().filter(|n| n.ty_name == Type::TYPE_NAME) {
            let schema = Type::from_body(&node.body)
                .map_err(|error| ResolutionError::Malformed { id: node.id, error })?;
            if schema.name == name {
                found.push((node.id, schema));
            }
        }
        if found.len() > 1 {
            return Err(ResolutionError::Ambiguous {
                name: name.into(),
                entries: found.into_iter().map(|(id, _)| id).collect(),
            });
        }
        Ok(found
            .pop()
            .map(|(id, schema)| TypeDefinition::Declared { id, schema }))
    }
}

// Declaration lists are flat, so unlike nested payloads their depth is not
// bounded by the codecs' recursion limits. Parsing, dropping, and comparing a
// `ValueType` recurse once per level, so a long run of `list` words would
// otherwise overflow the stack instead of being reported.
const MAX_SHAPE_DEPTH: usize = 64;

impl Type {
    pub const TYPE_NAME: &'static str = "Type";
    pub fn new(
        name: &str,
        fields: &[(&str, (ValueType, FieldStatus))],
        outgoing: &[&str],
        incoming: &[&str],
        successors: &[&str],
    ) -> Self {
        Self {
            name: name.into(),
            fields: fields
                .iter()
                .map(|(name, (ty, status))| {
                    (
                        name.to_string(),
                        FieldDecl {
                            ty: ty.clone(),
                            status: *status,
                        },
                    )
                })
                .collect(),
            outgoing: outgoing.iter().map(|s| s.to_string()).collect(),
            incoming: incoming.iter().map(|s| s.to_string()).collect(),
            successors: successors.iter().map(|s| s.to_string()).collect(),
        }
    }
    pub fn to_body(&self) -> Body {
        let strings = |values: &BTreeSet<String>| {
            Payload::List(values.iter().cloned().map(Payload::Text).collect())
        };
        Body::from([
            ("name".into(), self.name.clone().into()),
            ("fields".into(), fields_to_payload(&self.fields)),
            ("outgoing".into(), strings(&self.outgoing)),
            ("incoming".into(), strings(&self.incoming)),
            ("successors".into(), strings(&self.successors)),
        ])
    }
    pub fn from_body(body: &Body) -> Result<Self, SchemaError> {
        keys(
            body,
            &["name", "fields", "outgoing", "incoming", "successors"],
        )?;
        Ok(Self {
            name: text(required(body, "name")?)
                .map_err(|e| e.field("name"))?
                .into(),
            fields: match body.get("fields") {
            Some(v) => fields_from_payload(v, 0).map_err(|e| e.field("fields"))?,
            None => BTreeMap::new(),
            },
            outgoing: permissions(body, "outgoing")?,
            incoming: permissions(body, "incoming")?,
            successors: permissions(body, "successors")?,
        })
    }
    pub fn check(&self, body: &Body) -> Vec<FieldIssue> {
        let mut issues = vec![];
        check_fields(&self.fields, body, &mut vec![], &mut issues);
        issues
    }
}

impl ValueType {
    pub fn kind(&self) -> PayloadKind {
        match self {
        Self::Text => PayloadKind::Text,
        Self::Integer => PayloadKind::Integer,
        Self::Boolean => PayloadKind::Boolean,
        Self::Timestamp => PayloadKind::Timestamp,
        Self::List(_) => PayloadKind::List,
        Self::Record(_) => PayloadKind::Record,
        }
    }
    fn push_words(&self, words: &mut Vec<Payload>) {
        match self {
        Self::Text => words.push("text".into()),
        Self::Integer => words.push("integer".into()),
        Self::Boolean => words.push("boolean".into()),
        Self::Timestamp => words.push("timestamp".into()),
        Self::List(element) => {
            words.push("list".into());
            element.push_words(words);
        }
        Self::Record(fields) => {
            words.push("record".into());
            words.push(fields_to_payload(fields));
        }
        }
    }
    // Parses the shape that starts at `start` in a declaration list and returns
    // it with the index just after it. Indexes in errors are positions in that
    // list, so a caller adds only the field path. `depth` counts the `list` and
    // `record` words enclosing this shape, across nested records.
    fn from_words(
        words: &[Payload],
        start: usize,
        depth: usize,
    ) -> Result<(Self, usize), SchemaError> {
        let word = words
            .get(start)
            .ok_or_else(|| SchemaError::new(SchemaErrorKind::MissingShape).index(start))?;
        let word = text(word).map_err(|e| e.index(start))?;
        let next = start + 1;
        if matches!(word, "list" | "record") && depth >= MAX_SHAPE_DEPTH {
            return Err(SchemaError::new(SchemaErrorKind::TooDeep).index(start));
        }
        Ok(match word {
        "text" => (Self::Text, next),
        "integer" => (Self::Integer, next),
        "boolean" => (Self::Boolean, next),
        "timestamp" => (Self::Timestamp, next),
        "list" => {
            let (element, end) = Self::from_words(words, next, depth + 1)?;
            (Self::List(Box::new(element)), end)
        }
        "record" => {
            let fields = words.get(next).ok_or_else(|| {
                SchemaError::new(SchemaErrorKind::MissingRecordFields).index(next)
            })?;
            let fields = fields_from_payload(fields, depth + 1).map_err(|e| e.index(next))?;
            (Self::Record(fields), next + 1)
        }
        other => {
            return Err(SchemaError::new(SchemaErrorKind::UnknownShape(other.into())).index(start));
        }
        })
    }
    fn check_value(&self, value: &Payload, path: &mut Vec<PathPart>, issues: &mut Vec<FieldIssue>) {
        if self.kind() != value.kind() {
            issues.push(FieldIssue {
                path: path.clone(),
                kind: FieldIssueKind::TypeMismatch {
                    expected: self.kind(),
                    actual: value.kind(),
                },
            });
            return;
        }
        match (self, value) {
        (Self::List(element), Payload::List(values)) => {
            for (i, value) in values.iter().enumerate() {
                path.push(PathPart::Index(i));
                element.check_value(value, path, issues);
                path.pop();
            }
        }
        (Self::Record(fields), Payload::Record(body)) => check_fields(fields, body, path, issues),
        _ => {}
        }
    }
}

fn check_fields(
    fields: &BTreeMap<String, FieldDecl>,
    body: &Body,
    path: &mut Vec<PathPart>,
    issues: &mut Vec<FieldIssue>,
) {
    for (name, field) in fields {
        path.push(PathPart::Field(name.clone()));
        // Presence is membership, including empty lists and records. Shape is checked
        // separately.
        let value = body.get(name);
        let kind = match (field.status, value.is_some()) {
        (FieldStatus::Mandatory, false) => Some(FieldIssueKind::MissingMandatory),
        (FieldStatus::Encouraged, false) => Some(FieldIssueKind::MissingEncouraged),
        (FieldStatus::Deprecated, true) => Some(FieldIssueKind::Deprecated),
        _ => None,
        };
        if let Some(kind) = kind {
            issues.push(FieldIssue {
                path: path.clone(),
                kind,
            });
        }
        if let Some(value) = value {
            field.ty.check_value(value, path, issues);
        }
        path.pop();
    }
    for name in body.keys().filter(|name| !fields.contains_key(*name)) {
        path.push(PathPart::Field(name.clone()));
        issues.push(FieldIssue {
            path: path.clone(),
            kind: FieldIssueKind::UnexpectedField,
        });
        path.pop();
    }
}

fn fields_to_payload(fields: &BTreeMap<String, FieldDecl>) -> Payload {
    Payload::Record(
        fields
            .iter()
            .map(|(name, decl)| {
                let presence = match decl.status {
                FieldStatus::Mandatory => "mandatory",
                FieldStatus::Encouraged => "encouraged",
                FieldStatus::Deprecated => "deprecated",
                FieldStatus::Optional => "optional",
                };
                let mut words = vec![presence.into()];
                decl.ty.push_words(&mut words);
                (name.clone(), Payload::List(words))
            })
            .collect(),
    )
}

fn fields_from_payload(
    value: &Payload,
    depth: usize,
) -> Result<BTreeMap<String, FieldDecl>, SchemaError> {
    record(value)?
        .iter()
        .map(|(name, value)| {
            field_from_payload(value, depth)
                .map(|decl| (name.clone(), decl))
                .map_err(|e| e.field(name))
        })
        .collect()
}

// A declaration is `[presence, shape words...]`; a `record` word takes the next
// element as its sub-field declarations, so it ends the list.
fn field_from_payload(value: &Payload, depth: usize) -> Result<FieldDecl, SchemaError> {
    let Payload::List(words) = value else {
        return Err(wrong_shape(PayloadKind::List, value));
    };
    let presence = words
        .first()
        .ok_or_else(|| SchemaError::new(SchemaErrorKind::MissingPresence).index(0))?;
    let status = match text(presence).map_err(|e| e.index(0))? {
    "mandatory" => FieldStatus::Mandatory,
    "encouraged" => FieldStatus::Encouraged,
    "deprecated" => FieldStatus::Deprecated,
    "optional" => FieldStatus::Optional,
    other => {
        return Err(SchemaError::new(SchemaErrorKind::UnknownPresence(other.into())).index(0));
    }
    };
    let (ty, end) = ValueType::from_words(words, 1, depth)?;
    if end < words.len() {
        return Err(SchemaError::new(SchemaErrorKind::TrailingElements).index(end));
    }
    Ok(FieldDecl { ty, status })
}

fn keys(body: &Body, allowed: &[&str]) -> Result<(), SchemaError> {
    if let Some(key) = body.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(SchemaError::new(SchemaErrorKind::UnknownField).field(key));
    }
    Ok(())
}
fn required<'a>(body: &'a Body, name: &str) -> Result<&'a Payload, SchemaError> {
    body.get(name)
        .ok_or_else(|| SchemaError::new(SchemaErrorKind::MissingField).field(name))
}
fn record(value: &Payload) -> Result<&Body, SchemaError> {
    if let Payload::Record(body) = value {
        Ok(body)
    } else {
        Err(wrong_shape(PayloadKind::Record, value))
    }
}
fn text(value: &Payload) -> Result<&str, SchemaError> {
    if let Payload::Text(text) = value {
        Ok(text)
    } else {
        Err(wrong_shape(PayloadKind::Text, value))
    }
}
fn wrong_shape(expected: PayloadKind, value: &Payload) -> SchemaError {
    SchemaError::new(SchemaErrorKind::WrongShape {
        expected,
        actual: value.kind(),
    })
}
fn permissions(body: &Body, name: &str) -> Result<BTreeSet<String>, SchemaError> {
    let Some(value) = body.get(name) else {
        return Ok(BTreeSet::new());
    };
    let Payload::List(values) = value else {
        return Err(wrong_shape(PayloadKind::List, value).field(name));
    };
    let mut result = BTreeSet::new();
    for (i, value) in values.iter().enumerate() {
        let value = text(value).map_err(|e| e.index(i).field(name))?;
        if !result.insert(value.to_string()) {
            return Err(
                SchemaError::new(SchemaErrorKind::DuplicatePermission(value.into()))
                    .index(i)
                    .field(name),
            );
        }
    }
    Ok(result)
}
