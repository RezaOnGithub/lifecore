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
    Number,
    Boolean,
    PreciseTime,
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
    pub body: BTreeMap<String, FieldDecl>,
    pub outgoing: BTreeSet<String>,
    pub incoming: BTreeSet<String>,
    pub additional_override: BTreeSet<String>,
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
    UnknownKind(String),
    UnknownStatus(String),
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

impl Type {
    pub const TYPE_NAME: &'static str = "Type";
    pub fn new(
        name: &str,
        fields: &[(&str, (ValueType, FieldStatus))],
        outgoing: &[&str],
        incoming: &[&str],
        additional_override: &[&str],
    ) -> Self {
        Self {
            name: name.into(),
            body: fields
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
            additional_override: additional_override.iter().map(|s| s.to_string()).collect(),
        }
    }
    pub fn to_body(&self) -> Body {
        let strings = |values: &BTreeSet<String>| {
            Payload::List(values.iter().cloned().map(Payload::Text).collect())
        };
        Body::from([
            ("name".into(), self.name.clone().into()),
            ("body".into(), fields_to_payload(&self.body)),
            ("outgoing".into(), strings(&self.outgoing)),
            ("incoming".into(), strings(&self.incoming)),
            (
                "additional_override".into(),
                strings(&self.additional_override),
            ),
        ])
    }
    pub fn from_body(body: &Body) -> Result<Self, SchemaError> {
        keys(
            body,
            &[
                "name",
                "body",
                "outgoing",
                "incoming",
                "additional_override",
            ],
        )?;
        Ok(Self {
            name: text(required(body, "name")?)
                .map_err(|e| e.field("name"))?
                .into(),
            body: match body.get("body") {
            Some(v) => fields_from_payload(v).map_err(|e| e.field("body"))?,
            None => BTreeMap::new(),
            },
            outgoing: permissions(body, "outgoing")?,
            incoming: permissions(body, "incoming")?,
            additional_override: permissions(body, "additional_override")?,
        })
    }
    pub fn check(&self, body: &Body) -> Vec<FieldIssue> {
        let mut issues = vec![];
        check_fields(&self.body, body, &mut vec![], &mut issues);
        issues
    }
}

impl ValueType {
    pub fn kind(&self) -> PayloadKind {
        match self {
        Self::Text => PayloadKind::Text,
        Self::Number => PayloadKind::Number,
        Self::Boolean => PayloadKind::Boolean,
        Self::PreciseTime => PayloadKind::PreciseTime,
        Self::List(_) => PayloadKind::List,
        Self::Record(_) => PayloadKind::Record,
        }
    }
    fn to_payload(&self) -> Payload {
        let (kind, detail) = match self {
        Self::Text => ("text", None),
        Self::Number => ("integer", None),
        Self::Boolean => ("boolean", None),
        Self::PreciseTime => ("timestamp", None),
        Self::List(element) => ("list", Some(("element", element.to_payload()))),
        Self::Record(fields) => ("record", Some(("fields", fields_to_payload(fields)))),
        };
        let mut body = Body::from([("kind".into(), kind.into())]);
        if let Some((name, value)) = detail {
            body.insert(name.into(), value);
        }
        Payload::Record(body)
    }
    fn from_payload(value: &Payload) -> Result<Self, SchemaError> {
        let body = record(value)?;
        let kind = text(required(body, "kind")?).map_err(|e| e.field("kind"))?;
        let allowed = match kind {
        "list" => &["kind", "element"][..],
        "record" => &["kind", "fields"][..],
        _ => &["kind"][..],
        };
        keys(body, allowed)?;
        Ok(match kind {
        "text" => Self::Text,
        "integer" => Self::Number,
        "boolean" => Self::Boolean,
        "timestamp" => Self::PreciseTime,
        "list" => Self::List(Box::new(
            Self::from_payload(required(body, "element")?).map_err(|e| e.field("element"))?,
        )),
        "record" => Self::Record(
            fields_from_payload(required(body, "fields")?).map_err(|e| e.field("fields"))?,
        ),
        other => {
            return Err(SchemaError::new(SchemaErrorKind::UnknownKind(other.into())).field("kind"));
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
        //A Presence is membership, including empty lists and records. Shape is checked
        //A separately.
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
                let status = match decl.status {
                FieldStatus::Mandatory => "mandatory",
                FieldStatus::Encouraged => "encouraged",
                FieldStatus::Deprecated => "deprecated",
                FieldStatus::Optional => "optional",
                };
                (
                    name.clone(),
                    Payload::Record(Body::from([
                        ("shape".into(), decl.ty.to_payload()),
                        ("presence".into(), status.into()),
                    ])),
                )
            })
            .collect(),
    )
}

fn fields_from_payload(value: &Payload) -> Result<BTreeMap<String, FieldDecl>, SchemaError> {
    record(value)?
        .iter()
        .map(|(name, value)| {
            let parse = || {
                let body = record(value)?;
                keys(body, &["shape", "presence"])?;
                let status =
                    match text(required(body, "presence")?).map_err(|e| e.field("presence"))? {
                    "mandatory" => FieldStatus::Mandatory,
                    "encouraged" => FieldStatus::Encouraged,
                    "deprecated" => FieldStatus::Deprecated,
                    "optional" => FieldStatus::Optional,
                    other => {
                        return Err(
                            SchemaError::new(SchemaErrorKind::UnknownStatus(other.into()))
                                .field("presence"),
                        );
                    }
                    };
                Ok(FieldDecl {
                    ty: ValueType::from_payload(required(body, "shape")?)
                        .map_err(|e| e.field("shape"))?,
                    status,
                })
            };
            parse()
                .map(|decl| (name.clone(), decl))
                .map_err(|e: SchemaError| e.field(name))
        })
        .collect()
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
