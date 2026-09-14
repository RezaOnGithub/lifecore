use crate::{
    Edge, Entry, EntryId, Node, Payload, View, ViewError, ViewId,
    typing::{ResolutionError, TypeDefinition},
};
use serde::Serialize;
use std::fmt::Write;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub enum Currentness {
    #[default]
    Current,
    Historical,
    All,
}

impl Currentness {
    fn includes(self, view: View<'_>, id: EntryId) -> bool {
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

#[derive(Clone, Copy, Debug)]
pub enum Direction {
    Incoming,
    Outgoing,
    Both,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Interpretation {
    pub stale: bool,
    //A None is reserved for edges, whose type name is a relationship label.
    //A Nodes retain missing/malformed/ambiguous resolution as structured data.
    pub type_definition: Option<Result<Option<TypeDefinition>, ResolutionError>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InspectedEntry<'a> {
    pub entry: &'a Entry,
    pub interpretation: Interpretation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Inspection<'a> {
    pub view: ViewId,
    pub entries: Vec<InspectedEntry<'a>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InterpretationChange {
    pub entry: EntryId,
    pub before: Interpretation,
    pub after: Interpretation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Comparison<'a> {
    pub before: ViewId,
    pub after: ViewId,
    pub added: Vec<InspectedEntry<'a>>,
    pub changed: Vec<InterpretationChange>,
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
                FieldFilter::Present(name) => node.body.contains_key(*name),
                FieldFilter::Absent(name) => !node.body.contains_key(*name),
                FieldFilter::Equal(name, value) => node.body.get(*name) == Some(*value),
                })
                && selection
                    .text
                    .is_none_or(|text| node.body.values().any(|value| contains_text(value, text)))
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

    //A Descendants exclude the starting revision; ancestors and fresh_successors
    //A include it when eligible. All traversal retains exact revision identities.
    pub fn descendants(self, id: EntryId) -> impl Iterator<Item = &'a Node> {
        self.nodes().filter(move |node| {
            node.id != id && self.ancestors(node.id).skip(1).any(|n| n.id == id)
        })
    }

    pub fn inspect(self, id: EntryId) -> Option<InspectedEntry<'a>> {
        let entry = self.get(id)?;
        Some(InspectedEntry {
            entry,
            interpretation: Interpretation {
                stale: self.is_stale(id)?,
                type_definition: entry.as_node().map(|node| self.resolve_type(&node.ty_name)),
            },
        })
    }

    pub fn inspection(self) -> Inspection<'a> {
        Inspection {
            view: self.id(),
            entries: self
                .entries()
                .filter_map(|entry| self.inspect(entry.id()))
                .collect(),
        }
    }

    //A Comparison is directed: the later view must extend the entire earlier
    //A view. Sibling candidates share only their base, never provisional entries.
    pub fn compare<'b>(self, after: View<'b>) -> Result<Comparison<'b>, ViewError> {
        after.extends(self)?;
        let changed = self
            .entries()
            .filter_map(|entry| {
                let before = self.inspect(entry.id())?.interpretation;
                let later = after.inspect(entry.id())?.interpretation;
                (before != later).then_some(InterpretationChange {
                    entry: entry.id(),
                    before,
                    after: later,
                })
            })
            .collect();
        Ok(Comparison {
            before: self.id(),
            after: after.id(),
            added: after
                .entries()
                .skip(self.len())
                .filter_map(|entry| after.inspect(entry.id()))
                .collect(),
            changed,
        })
    }

    pub fn to_dot(self) -> String {
        let mut dot = String::from("digraph lifecore {\n");
        writeln!(
            dot,
            "  label=\"{} boundary {}\";",
            self.id().database,
            self.len()
        )
        .unwrap();
        for node in self.nodes() {
            let label = format!(
                "@{} {}\n{}",
                node.id.position(),
                node.ty_name,
                serde_json::to_string(&node.body).expect("payload JSON")
            );
            let color = if self.is_stale(node.id) == Some(true) {
                "gray"
            } else {
                "black"
            };
            writeln!(
                dot,
                "  n{} [label={}, color={color}];",
                node.id.position(),
                dot_quote(&label)
            )
            .unwrap();
        }
        for node in self.nodes() {
            if let Some(parent) = node.parent {
                writeln!(
                    dot,
                    "  n{} -> n{} [label=\"revision\", style=dashed, color=blue];",
                    parent.position(),
                    node.id.position()
                )
                .unwrap();
            }
        }
        for edge in self.edges() {
            let color = if self.is_stale(edge.id) == Some(true) {
                "gray"
            } else {
                "black"
            };
            let label = format!("@{} {}", edge.id.position(), edge.ty_name);
            writeln!(
                dot,
                "  n{} -> n{} [label={}, style=solid, color={color}];",
                edge.from.position(),
                edge.to.position(),
                dot_quote(&label)
            )
            .unwrap();
        }
        dot.push_str("}\n");
        dot
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

fn dot_quote(text: &str) -> String {
    //A Escape backslashes first, including Graphviz's own label substitutions.
    format!(
        "\"{}\"",
        text.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
            .replace('\t', "\\t")
    )
}

#[cfg(test)]
mod test {
    mod test2;
}
