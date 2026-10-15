use crate::{
    AppError,
    proposal::present,
    schema::{self, TypeResolution},
    service::EntryRecord,
};
use lifecore::{
    Edge, Entry, EntryId, Node, View, ViewId,
    inspection::{Currentness, Direction, EdgeSelection, NodeSelection},
};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroUsize,
};

// The side of the inspected node an incident edge is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Incidence {
    Outgoing,
    Incoming,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncidentEdge {
    pub edge: EntryRecord,
    pub direction: Incidence,
    pub other: EntryRecord,
}

// A node with its type, its whole revision tree in append order (root first,
// itself included), and every incident edge of any currentness in append
// order; or an edge with its endpoints.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntryInspection {
    Node {
        view: ViewId,
        entry: EntryRecord,
        #[serde(rename = "type")]
        ty: TypeResolution,
        lineage: Vec<EntryRecord>,
        edges: Vec<IncidentEdge>,
    },
    Edge {
        view: ViewId,
        entry: EntryRecord,
        from: EntryRecord,
        to: EntryRecord,
    },
}

// An edge's endpoints precede it, so a visible edge has visible node
// endpoints; anything else is a broken graph, not an absent entry.
fn endpoint(view: View<'_>, id: EntryId) -> Result<&Node, AppError> {
    view.node(id).ok_or_else(|| AppError::Internal {
        message: format!("edge endpoint {id:?} is not a node of the view"),
    })
}

pub fn inspect(view: View<'_>, id: EntryId) -> Result<EntryInspection, AppError> {
    let entry = view.get(id).ok_or(AppError::EntryNotFound { id })?;
    Ok(match entry {
    Entry::Node(node) => {
        // The root is the last ancestor, or the node itself when it has none.
        let root = view.ancestors(id).last().map_or(id, |ancestor| ancestor.id);
        // A predecessor precedes its successors, so one pass in append order
        // meets every member's predecessor before the member itself: a node
        // is in the tree exactly when it is the root or its predecessor is.
        let mut members = BTreeSet::from([root]);
        let mut lineage = vec![];
        for other in view.nodes() {
            if other.id == root
                || other
                    .predecessor
                    .is_some_and(|predecessor| members.contains(&predecessor))
            {
                members.insert(other.id);
                lineage.push(EntryRecord::node(view, other));
            }
        }
        let mut edges = vec![];
        for edge in view.edges_of(id) {
            let (direction, other) = if edge.from == id {
                (Incidence::Outgoing, edge.to)
            } else {
                (Incidence::Incoming, edge.from)
            };
            edges.push(IncidentEdge {
                edge: EntryRecord::edge(view, edge),
                direction,
                other: EntryRecord::node(view, endpoint(view, other)?),
            });
        }
        EntryInspection::Node {
            view: view.id(),
            entry: EntryRecord::node(view, node),
            ty: schema::resolve_name(view, &node.ty_name),
            lineage,
            edges,
        }
    }
    Entry::Edge(edge) => EntryInspection::Edge {
        view: view.id(),
        entry: EntryRecord::edge(view, edge),
        from: EntryRecord::node(view, endpoint(view, edge.from)?),
        to: EntryRecord::node(view, endpoint(view, edge.to)?),
    },
    })
}

// Where a query starts: exact node identities, or a node selection.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Seed {
    Ids {
        ids: Vec<EntryId>,
    },
    Nodes {
        #[serde(default, deserialize_with = "present")]
        ty_name: Option<String>,
        #[serde(default)]
        currentness: Currentness,
        #[serde(default, deserialize_with = "present")]
        text: Option<String>,
    },
}

// One expansion step: follow matching edges `hops` times from the frontier.
// A step without hops is refused when decoded, before any view is selected.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expansion {
    #[serde(default = "one")]
    pub hops: NonZeroUsize,
    #[serde(default)]
    pub direction: Direction,
    #[serde(default, deserialize_with = "ty_names")]
    pub ty_names: Option<Vec<String>>,
    #[serde(default)]
    pub currentness: Currentness,
}

fn one() -> NonZeroUsize {
    NonZeroUsize::MIN
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EdgeFilter {
    #[serde(default, deserialize_with = "ty_names")]
    pub ty_names: Option<Vec<String>>,
    #[serde(default)]
    pub currentness: Currentness,
}

// Absent means any relationship type. An empty list would match nothing, so
// it is refused when decoded rather than answered with an empty result.
fn ty_names<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Vec<String>>, D::Error> {
    let names = Vec::<String>::deserialize(deserializer)?;
    if names.is_empty() {
        return Err(D::Error::custom(
            "an empty ty_names list matches nothing; omit it for any type",
        ));
    }
    Ok(Some(names))
}

// Why a node is in the result: a seed, or first reached at `hop` over the
// edge `via` from the node `from`. Hops count from the seeds across steps.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Provenance {
    Seed,
    Reached {
        hop: usize,
        via: EntryId,
        from: EntryId,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueriedNode {
    pub node: EntryRecord,
    pub provenance: Provenance,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewQuery {
    pub view: ViewId,
    pub nodes: Vec<QueriedNode>,
    pub edges: Vec<EntryRecord>,
}

fn matches(ty_names: Option<&[String]>, ty_name: &str) -> bool {
    ty_names.is_none_or(|names| names.iter().any(|name| name == ty_name))
}

// Seeds, then each step breadth first. A hop reads the frontier's incident
// edges and crosses, in append order, every matching one that joins a
// frontier node to a node not yet found, so the earliest edge decides a
// node's provenance whatever order the seeds were given in. The frontier
// after a step is the nodes its last hop added. Edges are then the matching
// ones among the result nodes.
pub fn query<'a>(
    view: View<'a>,
    seeds: &[Seed],
    expand: &[Expansion],
    filter: &EdgeFilter,
) -> Result<ViewQuery, AppError> {
    // Keyed by position, which is append order within one view.
    let mut found = BTreeMap::<usize, (&Node, Provenance)>::new();
    let mut frontier = BTreeSet::new();
    let mut seed = |node: &'a Node, frontier: &mut BTreeSet<EntryId>| {
        if found
            .insert(node.id.position(), (node, Provenance::Seed))
            .is_none()
        {
            frontier.insert(node.id);
        }
    };
    for selection in seeds {
        match selection {
        Seed::Ids { ids } => {
            for &id in ids {
                let node = view.node(id).ok_or(AppError::EntryNotFound { id })?;
                seed(node, &mut frontier);
            }
        }
        Seed::Nodes {
            ty_name,
            currentness,
            text,
        } => {
            let selection = NodeSelection {
                ty_name: ty_name.as_deref(),
                currentness: *currentness,
                fields: vec![],
                text: text.as_deref(),
            };
            for node in view.select_nodes(&selection) {
                seed(node, &mut frontier);
            }
        }
        }
    }
    // Every edge of the view under each of its endpoints, in append order
    // within each list, so a hop reads only the frontier's incident edges.
    let mut incident = BTreeMap::<EntryId, Vec<&'a Edge>>::new();
    for edge in view.edges() {
        incident.entry(edge.from).or_default().push(edge);
        if edge.to != edge.from {
            incident.entry(edge.to).or_default().push(edge);
        }
    }
    let mut hop = 0;
    for step in expand {
        for _ in 0..step.hops.get() {
            if frontier.is_empty() {
                break;
            }
            hop += 1;
            // Candidate crossings are gathered per frontier node and then
            // applied in edge order, so the earliest edge still decides the
            // provenance of a node several frontier nodes reach.
            let mut crossings = vec![];
            for &node in &frontier {
                for edge in incident.get(&node).into_iter().flatten().copied() {
                    if !matches(step.ty_names.as_deref(), &edge.ty_name)
                        || !step.currentness.includes(view, edge.id)
                    {
                        continue;
                    }
                    let reached = match step.direction {
                    Direction::Outgoing if edge.from == node => edge.to,
                    Direction::Incoming if edge.to == node => edge.from,
                    Direction::Both if edge.from == node => edge.to,
                    Direction::Both => edge.from,
                    Direction::Outgoing | Direction::Incoming => continue,
                    };
                    crossings.push((edge.id.position(), edge, node, reached));
                }
            }
            crossings.sort_by_key(|(position, ..)| *position);
            let mut next = BTreeSet::new();
            for (_, edge, from, reached) in crossings {
                if found.contains_key(&reached.position()) {
                    continue;
                }
                let provenance = Provenance::Reached {
                    hop,
                    via: edge.id,
                    from,
                };
                found.insert(reached.position(), (endpoint(view, reached)?, provenance));
                next.insert(reached);
            }
            frontier = next;
        }
    }
    let selection = EdgeSelection {
        ty_name: None,
        currentness: filter.currentness,
    };
    let edges = view
        .select_edges(selection)
        .filter(|edge| {
            matches(filter.ty_names.as_deref(), &edge.ty_name)
                && found.contains_key(&edge.from.position())
                && found.contains_key(&edge.to.position())
        })
        .map(|edge| EntryRecord::edge(view, edge))
        .collect();
    Ok(ViewQuery {
        view: view.id(),
        nodes: found
            .into_values()
            .map(|(node, provenance)| QueriedNode {
                node: EntryRecord::node(view, node),
                provenance,
            })
            .collect(),
        edges,
    })
}
