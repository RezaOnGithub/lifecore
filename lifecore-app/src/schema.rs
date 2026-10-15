use lifecore::{
    EntryId, Payload, View, ViewId,
    typing::{self, SchemaError, Type, ValueType},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

// The structured form of a declaration: the same information as its
// word-list payload (FILES 2.6), spelled for a client that renders it rather
// than parses it. The core `typing` serde is untouched.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldStatus {
    Mandatory,
    Encouraged,
    Deprecated,
    Optional,
}

impl From<typing::FieldStatus> for FieldStatus {
    fn from(status: typing::FieldStatus) -> Self {
        match status {
        typing::FieldStatus::Mandatory => Self::Mandatory,
        typing::FieldStatus::Encouraged => Self::Encouraged,
        typing::FieldStatus::Deprecated => Self::Deprecated,
        typing::FieldStatus::Optional => Self::Optional,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Shape {
    Text,
    Integer,
    Boolean,
    Timestamp,
    List { element: Box<Shape> },
    Record { fields: BTreeMap<String, FieldDecl> },
}

impl From<&ValueType> for Shape {
    fn from(ty: &ValueType) -> Self {
        match ty {
        ValueType::Text => Self::Text,
        ValueType::Integer => Self::Integer,
        ValueType::Boolean => Self::Boolean,
        ValueType::Timestamp => Self::Timestamp,
        ValueType::List(element) => Self::List {
            element: Box::new(element.as_ref().into()),
        },
        ValueType::Record(fields) => Self::Record {
            fields: field_decls(fields),
        },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldDecl {
    pub status: FieldStatus,
    pub shape: Shape,
}

fn field_decls(fields: &BTreeMap<String, typing::FieldDecl>) -> BTreeMap<String, FieldDecl> {
    fields
        .iter()
        .map(|(name, decl)| {
            (
                name.clone(),
                FieldDecl {
                    status: decl.status.into(),
                    shape: (&decl.ty).into(),
                },
            )
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeSchema {
    pub name: String,
    pub fields: BTreeMap<String, FieldDecl>,
    pub outgoing: BTreeSet<String>,
    pub incoming: BTreeSet<String>,
    pub successors: BTreeSet<String>,
}

impl From<&Type> for TypeSchema {
    fn from(ty: &Type) -> Self {
        Self {
            name: ty.name.clone(),
            fields: field_decls(&ty.fields),
            outgoing: ty.outgoing.clone(),
            incoming: ty.incoming.clone(),
            successors: ty.successors.clone(),
        }
    }
}

// How a name resolves in the selected view (SPEC 6.2), tagged by `kind`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TypeResolution {
    Builtin,
    Declared {
        declaration: EntryId,
        schema: TypeSchema,
    },
    Missing,
    Ambiguous {
        declarations: Vec<EntryId>,
    },
    Malformed {
        declaration: EntryId,
        message: String,
    },
}

type Declaration = (EntryId, Result<Type, SchemaError>);

// The current declarations of a view grouped by the `name` text of their
// payloads, in append order within each name. A declaration that does not
// parse is grouped under its `name` text when it has one, else under the
// empty string.
struct Declarations(BTreeMap<String, Vec<Declaration>>);

impl Declarations {
    fn current(view: View<'_>) -> Self {
        let mut grouped = BTreeMap::<String, Vec<Declaration>>::new();
        for node in view
            .fresh_nodes()
            .filter(|node| node.ty_name == Type::TYPE_NAME)
        {
            let parsed = Type::from_body(&node.payload);
            let name = match (&parsed, node.payload.get("name")) {
            (Ok(schema), _) => schema.name.clone(),
            (Err(_), Some(Payload::Text(name))) => name.clone(),
            (Err(_), _) => String::new(),
            };
            grouped.entry(name).or_default().push((node.id, parsed));
        }
        Self(grouped)
    }

    fn names(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }

    // Judges a name by its own current declarations (SPEC 6.2), unlike
    // `View::resolve_type`, which fails every name once any current
    // declaration is malformed. Two or more declarations are ambiguous whether
    // or not they parse: the name has no single declaration to read.
    fn resolve(&self, name: &str) -> TypeResolution {
        if name == Type::TYPE_NAME {
            return TypeResolution::Builtin;
        }
        match self.0.get(name).map(Vec::as_slice) {
        None | Some([]) => TypeResolution::Missing,
        Some([(id, Ok(schema))]) => TypeResolution::Declared {
            declaration: *id,
            schema: schema.into(),
        },
        Some([(id, Err(error))]) => TypeResolution::Malformed {
            declaration: *id,
            message: error.to_string(),
        },
        Some(found) => TypeResolution::Ambiguous {
            declarations: found.iter().map(|(id, _)| *id).collect(),
        },
        }
    }
}

// How one name resolves in the view, judged by its own declarations.
pub fn resolve_name(view: View<'_>, name: &str) -> TypeResolution {
    Declarations::current(view).resolve(name)
}

// Nodes whose `ty_name` is the summarized name, by currentness in the view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeCounts {
    #[serde(with = "lifecore::decimal")]
    pub current: usize,
    #[serde(with = "lifecore::decimal")]
    pub historical: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeSummary {
    pub name: String,
    pub resolution: TypeResolution,
    pub nodes: NodeCounts,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeList {
    pub view: ViewId,
    pub types: Vec<TypeSummary>,
}

// One summary per name the view knows, sorted by name: the built-in `Type`,
// every name with a current declaration, and every name a node uses, each
// resolved by its own declarations as `resolve_name` does.
pub fn list_types(view: View<'_>) -> TypeList {
    let mut counts = BTreeMap::<String, NodeCounts>::new();
    for node in view.nodes() {
        let counts = counts.entry(node.ty_name.clone()).or_default();
        if view.is_stale(node.id) == Some(true) {
            counts.historical += 1;
        } else {
            counts.current += 1;
        }
    }
    let declarations = Declarations::current(view);
    let mut names = BTreeSet::from([Type::TYPE_NAME.to_string()]);
    names.extend(counts.keys().cloned());
    names.extend(declarations.names().map(str::to_string));
    let types = names
        .into_iter()
        .map(|name| {
            let resolution = declarations.resolve(&name);
            let nodes = counts.remove(&name).unwrap_or_default();
            TypeSummary {
                name,
                resolution,
                nodes,
            }
        })
        .collect();
    TypeList {
        view: view.id(),
        types,
    }
}
