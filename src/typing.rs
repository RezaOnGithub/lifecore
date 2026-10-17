use crate::{Body, EntryId, Payload, PayloadKind, View};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldStatus {
    Mandatory,
    Encouraged,
    Deprecated,
    Optional,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ValueType {
    Text,
    Integer,
    Boolean,
    Timestamp,
    List { element: Box<ValueType> },
    Record { fields: BTreeMap<String, FieldDecl> },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FieldDecl {
    pub status: FieldStatus,
    pub shape: ValueType,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Type {
    pub name: String,
    pub fields: BTreeMap<String, FieldDecl>,
    pub outgoing: BTreeSet<String>,
    pub incoming: BTreeSet<String>,
    pub successors: BTreeSet<String>,
}

// A field name or a list position; the two never share a JSON type, so the
// untagged form is unambiguous.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
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
pub struct SchemaError {
    pub path: Vec<PathPart>,
    #[serde(flatten)]
    pub kind: SchemaErrorKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SchemaErrorKind {
    MissingField,
    UnknownField,
    WrongShape {
        expected: PayloadKind,
        actual: PayloadKind,
    },
    MissingPresence,
    UnknownPresence {
        word: String,
    },
    MissingShape,
    UnknownShape {
        word: String,
    },
    MissingRecordFields,
    TrailingElements,
    TooDeep,
    DuplicatePermission {
        name: String,
    },
}

// A payload path as messages write it: `email`, `details.owner`, `tags[1]`.
// A field name outside the identifier grammar is quoted, so a dot, bracket, or
// space inside a name cannot be read as structure.
pub fn path_text(path: &[PathPart]) -> String {
    let mut text = String::new();
    for part in path {
        match part {
        PathPart::Field(name) => {
            if !text.is_empty() {
                text.push('.');
            }
            let mut chars = name.chars();
            let plain = chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
            if plain {
                text.push_str(name);
            } else {
                text.push_str(&format!("{name:?}"));
            }
        }
        PathPart::Index(index) => text.push_str(&format!("[{index}]")),
        }
    }
    text
}

// The declaration word of a payload kind, as in "should be integer, found text".
pub fn kind_word(kind: PayloadKind) -> &'static str {
    match kind {
    PayloadKind::Text => "text",
    PayloadKind::Integer => "integer",
    PayloadKind::Boolean => "boolean",
    PayloadKind::Timestamp => "timestamp",
    PayloadKind::List => "list",
    PayloadKind::Record => "record",
    }
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.path.is_empty() {
            f.write_str("invalid schema: ")?;
        } else {
            write!(f, "invalid schema at {}: ", path_text(&self.path))?;
        }
        match &self.kind {
        SchemaErrorKind::MissingField => f.write_str("field is missing"),
        SchemaErrorKind::UnknownField => f.write_str("unknown field"),
        SchemaErrorKind::WrongShape { expected, actual } => write!(
            f,
            "should be {}, found {}",
            kind_word(*expected),
            kind_word(*actual)
        ),
        SchemaErrorKind::MissingPresence => f.write_str("missing presence word"),
        SchemaErrorKind::UnknownPresence { word } => write!(f, "unknown presence word {word:?}"),
        SchemaErrorKind::MissingShape => f.write_str("missing shape word"),
        SchemaErrorKind::UnknownShape { word } => write!(f, "unknown shape word {word:?}"),
        SchemaErrorKind::MissingRecordFields => {
            f.write_str("missing field declarations after \"record\"")
        }
        SchemaErrorKind::TrailingElements => f.write_str("unexpected words after the shape"),
        SchemaErrorKind::TooDeep => {
            write!(f, "shape nested more than {MAX_SHAPE_DEPTH} levels deep")
        }
        SchemaErrorKind::DuplicatePermission { name } => {
            write!(f, "duplicate permission {name:?}")
        }
        }
    }
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

// One current declaration and how its payload parses.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Declaration {
    Parsed {
        declaration: EntryId,
        schema: Type,
    },
    Malformed {
        declaration: EntryId,
        error: SchemaError,
    },
}

impl Declaration {
    pub fn id(&self) -> EntryId {
        match self {
        Self::Parsed { declaration, .. } | Self::Malformed { declaration, .. } => *declaration,
        }
    }
    pub fn schema(&self) -> Result<&Type, &SchemaError> {
        match self {
        Self::Parsed { schema, .. } => Ok(schema),
        Self::Malformed { error, .. } => Err(error),
        }
    }
}

// How a type name resolves in a view (SPEC 6.2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TypeResolution {
    Builtin,
    Declared {
        declaration: EntryId,
        schema: Type,
    },
    Missing,
    Malformed {
        declaration: EntryId,
        error: SchemaError,
    },
    // Every claimant, in append order.
    Ambiguous {
        claimants: Vec<Declaration>,
    },
}

// The current declarations of one view, indexed by the name each claims.
// Derived from that view alone; nothing caches or serializes it.
#[derive(Clone, Debug)]
pub struct Declarations<'a> {
    // Append order, each with the `name` text it claims, if any.
    all: Vec<(Option<&'a str>, Declaration)>,
    // Indexes into `all`, in append order. A declaration without a text
    // `name` claims nothing, so it is never filed under `""`, which is a
    // legal type name.
    claims: BTreeMap<&'a str, Vec<usize>>,
}

impl<'a> Declarations<'a> {
    pub fn iter(&self) -> impl Iterator<Item = (Option<&'a str>, &Declaration)> {
        self.all
            .iter()
            .map(|(name, declaration)| (*name, declaration))
    }
    // Every claimed name in order, including a claimed `Type`.
    pub fn names(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.claims.keys().copied()
    }
    pub fn claimants(&self, name: &str) -> impl Iterator<Item = &Declaration> {
        self.claimed(name).iter().map(|&i| &self.all[i].1)
    }
    pub fn nameless(&self) -> impl Iterator<Item = &Declaration> {
        self.iter()
            .filter(|(name, _)| name.is_none())
            .map(|(_, declaration)| declaration)
    }
    fn claimed(&self, name: &str) -> &[usize] {
        self.claims.get(name).map_or(&[], Vec::as_slice)
    }
    // Only a name's own claimants decide it: a malformed declaration under
    // another name, or under none, affects nothing here.
    pub fn resolve(&self, name: &str) -> TypeResolution {
        if name == Type::TYPE_NAME {
            return TypeResolution::Builtin;
        }
        match self.claimed(name) {
        [] => TypeResolution::Missing,
        [only] => match &self.all[*only].1 {
        Declaration::Parsed {
            declaration,
            schema,
        } => TypeResolution::Declared {
            declaration: *declaration,
            schema: schema.clone(),
        },
        Declaration::Malformed { declaration, error } => TypeResolution::Malformed {
            declaration: *declaration,
            error: error.clone(),
        },
        },
        _ => TypeResolution::Ambiguous {
            claimants: self.claimants(name).cloned().collect(),
        },
        }
    }
    // The schema exactly when `resolve` answers `Declared`, without a copy.
    pub fn declared(&self, name: &str) -> Option<(EntryId, &Type)> {
        if name == Type::TYPE_NAME {
            return None;
        }
        match self.claimed(name) {
        [only] => match &self.all[*only].1 {
        Declaration::Parsed {
            declaration,
            schema,
        } => Some((*declaration, schema)),
        Declaration::Malformed { .. } => None,
        },
        _ => None,
        }
    }
    // Exactly when `resolve` answers `Missing`.
    pub fn is_missing(&self, name: &str) -> bool {
        name != Type::TYPE_NAME && self.claimed(name).is_empty()
    }
}

impl<'a> View<'a> {
    pub fn declarations(self) -> Declarations<'a> {
        let mut index = Declarations {
            all: vec![],
            claims: BTreeMap::new(),
        };
        for node in self
            .fresh_nodes()
            .filter(|node| node.ty_name == Type::TYPE_NAME)
        {
            // The claim reads the `name` text alone, so a declaration whose
            // other keys do not parse still claims its name.
            let name = match node.payload.get("name") {
            Some(Payload::Text(name)) => Some(name.as_str()),
            _ => None,
            };
            let declaration = match Type::from_body(&node.payload) {
            Ok(schema) => Declaration::Parsed {
                declaration: node.id,
                schema,
            },
            Err(error) => Declaration::Malformed {
                declaration: node.id,
                error,
            },
            };
            if let Some(name) = name {
                index.claims.entry(name).or_default().push(index.all.len());
            }
            index.all.push((name, declaration));
        }
        index
    }
    pub fn resolve_type(self, name: &str) -> TypeResolution {
        self.declarations().resolve(name)
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
                            status: *status,
                            shape: ty.clone(),
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
        Self::List { .. } => PayloadKind::List,
        Self::Record { .. } => PayloadKind::Record,
        }
    }
    fn push_words(&self, words: &mut Vec<Payload>) {
        match self {
        Self::Text => words.push("text".into()),
        Self::Integer => words.push("integer".into()),
        Self::Boolean => words.push("boolean".into()),
        Self::Timestamp => words.push("timestamp".into()),
        Self::List { element } => {
            words.push("list".into());
            element.push_words(words);
        }
        Self::Record { fields } => {
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
            (
                Self::List {
                    element: Box::new(element),
                },
                end,
            )
        }
        "record" => {
            let fields = words.get(next).ok_or_else(|| {
                SchemaError::new(SchemaErrorKind::MissingRecordFields).index(next)
            })?;
            let fields = fields_from_payload(fields, depth + 1).map_err(|e| e.index(next))?;
            (Self::Record { fields }, next + 1)
        }
        other => {
            return Err(
                SchemaError::new(SchemaErrorKind::UnknownShape { word: other.into() }).index(start),
            );
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
        (Self::List { element }, Payload::List(values)) => {
            for (i, value) in values.iter().enumerate() {
                path.push(PathPart::Index(i));
                element.check_value(value, path, issues);
                path.pop();
            }
        }
        (Self::Record { fields }, Payload::Record(body)) => {
            check_fields(fields, body, path, issues)
        }
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
            field.shape.check_value(value, path, issues);
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
                decl.shape.push_words(&mut words);
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
        return Err(
            SchemaError::new(SchemaErrorKind::UnknownPresence { word: other.into() }).index(0),
        );
    }
    };
    let (ty, end) = ValueType::from_words(words, 1, depth)?;
    if end < words.len() {
        return Err(SchemaError::new(SchemaErrorKind::TrailingElements).index(end));
    }
    Ok(FieldDecl { status, shape: ty })
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
            return Err(SchemaError::new(SchemaErrorKind::DuplicatePermission {
                name: value.into(),
            })
            .index(i)
            .field(name));
        }
    }
    Ok(result)
}
