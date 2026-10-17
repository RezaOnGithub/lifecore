use crate::{
    Edge, Entry, EntryId, Node, Payload, View, ViewError, ViewId,
    typing::{ResolutionError, TypeDefinition},
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Currentness {
    #[default]
    Current,
    Historical,
    All,
}

impl Currentness {
    pub fn includes(self, view: View<'_>, id: EntryId) -> bool {
        match (self, view.is_stale(id)) {
        (_, None) => false,
        (Self::All, _) => true,
        (Self::Current, Some(stale)) => !stale,
        (Self::Historical, Some(stale)) => stale,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum FieldFilter<'a> {
    Present(&'a str),
    Absent(&'a str),
    Equal(&'a str, &'a Payload),
}

#[derive(Clone, Debug, Default)]
pub struct NodeSelection<'a> {
    pub ty_name: Option<&'a str>,
    pub currentness: Currentness,
    pub fields: Vec<FieldFilter<'a>>,
    pub text: Option<&'a str>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct EdgeSelection<'a> {
    pub ty_name: Option<&'a str>,
    pub currentness: Currentness,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Incoming,
    Outgoing,
    #[default]
    Both,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Resolution<'a> {
    pub entry: &'a Entry,
    pub stale: bool,
    pub type_definition: TypeResolution,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeResolution {
    // Edges carry relationship labels rather than node type declarations.
    NotApplicableToEdge,
    MissingDeclaration,
    Resolved(TypeDefinition),
    Failed(ResolutionError),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Inspection<'a> {
    pub view: ViewId,
    pub entries: Vec<Resolution<'a>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ResolutionDifference<'base, 'target> {
    at_base: Resolution<'base>,
    at_target: Resolution<'target>,
}

impl<'base, 'target> ResolutionDifference<'base, 'target> {
    // Only distinct resolutions of the same immutable entry form a difference.
    // Private fields and shared accessors preserve that invariant after construction.
    pub fn new(at_base: Resolution<'base>, at_target: Resolution<'target>) -> Option<Self> {
        if at_base.entry != at_target.entry || at_base == at_target {
            return None;
        }
        Some(Self { at_base, at_target })
    }

    pub fn at_base(&self) -> &Resolution<'base> {
        &self.at_base
    }

    pub fn at_target(&self) -> &Resolution<'target> {
        &self.at_target
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ResolutionComparison<'base, 'target> {
    pub base_view: ViewId,
    pub target_view: ViewId,
    pub added: Vec<Resolution<'target>>,
    pub changed: Vec<ResolutionDifference<'base, 'target>>,
}

impl<'a> View<'a> {
    pub fn select_nodes<'s>(
        self,
        selection: &'s NodeSelection<'s>,
    ) -> impl Iterator<Item = &'a Node> + 's
    where
        'a: 's,
    {
        self.nodes().filter(move |node| {
            selection.ty_name.is_none_or(|name| name == node.ty_name)
                && selection.currentness.includes(self, node.id)
                && selection.fields.iter().all(|filter| match filter {
                FieldFilter::Present(name) => node.payload.contains_key(*name),
                FieldFilter::Absent(name) => !node.payload.contains_key(*name),
                FieldFilter::Equal(name, value) => node.payload.get(*name) == Some(*value),
                })
                && selection.text.is_none_or(|text| {
                    node.payload
                        .values()
                        .any(|value| contains_text(value, text))
                })
        })
    }

    pub fn select_edges<'s>(
        self,
        selection: EdgeSelection<'s>,
    ) -> impl Iterator<Item = &'a Edge> + 's
    where
        'a: 's,
    {
        self.edges().filter(move |edge| {
            selection.ty_name.is_none_or(|name| name == edge.ty_name)
                && selection.currentness.includes(self, edge.id)
        })
    }

    pub fn relationships<'s>(
        self,
        node: EntryId,
        direction: Direction,
        selection: EdgeSelection<'s>,
    ) -> impl Iterator<Item = &'a Edge> + 's
    where
        'a: 's,
    {
        self.select_edges(selection)
            .filter(move |edge| match direction {
            Direction::Incoming => edge.to == node,
            Direction::Outgoing => edge.from == node,
            Direction::Both => edge.from == node || edge.to == node,
            })
    }

    // Ancestors and descendants exclude the starting revision. Fresh successors
    // include it when current. All traversal retains exact revision identities.
    // A predecessor precedes its successors, so one pass in append order meets
    // each member of the tree below `id` after its predecessor.
    pub fn descendants(self, id: EntryId) -> impl Iterator<Item = &'a Node> {
        let start = self.node(id);
        let mut members = start
            .map(|start| start.id)
            .into_iter()
            .collect::<HashSet<_>>();
        self.entries()
            .skip(start.map_or(self.len(), |start| start.id.position() + 1))
            .filter_map(Entry::as_node)
            .filter(move |node| {
                let member = node
                    .predecessor
                    .is_some_and(|predecessor| members.contains(&predecessor));
                if member {
                    members.insert(node.id);
                }
                member
            })
    }

    pub fn resolve(self, id: EntryId) -> Option<Resolution<'a>> {
        let entry = self.get(id)?;
        Some(Resolution {
            entry,
            stale: self.is_stale(id)?,
            type_definition: match entry {
            Entry::Edge(_) => TypeResolution::NotApplicableToEdge,
            Entry::Node(node) => match self.resolve_type(&node.ty_name) {
            Ok(Some(definition)) => TypeResolution::Resolved(definition),
            Ok(None) => TypeResolution::MissingDeclaration,
            Err(error) => TypeResolution::Failed(error),
            },
            },
        })
    }

    pub fn inspection(self) -> Inspection<'a> {
        Inspection {
            view: self.id(),
            entries: self
                .entries()
                .filter_map(|entry| self.resolve(entry.id()))
                .collect(),
        }
    }

    // The target must extend the entire base, including entry contents. Added
    // positions are [base_view.end_exclusive, target_view.end_exclusive).
    pub fn compare<'target>(
        self,
        target: View<'target>,
    ) -> Result<ResolutionComparison<'a, 'target>, ViewError> {
        target.extends(self)?;
        let changed = self
            .entries()
            .filter_map(|entry| {
                let at_base = self.resolve(entry.id())?;
                let at_target = target.resolve(entry.id())?;
                ResolutionDifference::new(at_base, at_target)
            })
            .collect();
        Ok(ResolutionComparison {
            base_view: self.id(),
            target_view: target.id(),
            added: target
                .entries()
                .skip(self.len())
                .filter_map(|entry| target.resolve(entry.id()))
                .collect(),
            changed,
        })
    }
}

fn contains_text(value: &Payload, needle: &str) -> bool {
    match value {
    Payload::Text(text) => text.contains(needle),
    Payload::List(values) => values.iter().any(|value| contains_text(value, needle)),
    Payload::Record(fields) => fields.values().any(|value| contains_text(value, needle)),
    _ => false,
    }
}

#[cfg(test)]
mod test;
