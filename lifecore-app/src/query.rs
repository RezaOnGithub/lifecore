use crate::{AppError, proposal::present, schema::TypeResolution, service::EntryRecord};
use lifecore::{
    Entry, EntryId, Node, View, ViewId,
    inspection::{Currentness, Direction, EdgeSelection, NodeSelection},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

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
        let root = view.root_of(id);
        let lineage = view
            .nodes()
            .filter(|other| view.root_of(other.id) == root)
            .map(|other| EntryRecord::node(view, other))
            .collect();
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
            ty: TypeResolution::resolve(view, &node.ty_name),
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
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expansion {
    #[serde(default = "one")]
    pub hops: usize,
    #[serde(default)]
    pub direction: Direction,
    // Absent means any relationship type.
    #[serde(default, deserialize_with = "present")]
    pub ty_names: Option<Vec<String>>,
    #[serde(default)]
    pub currentness: Currentness,
}

fn one() -> usize {
    1
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EdgeFilter {
    #[serde(default, deserialize_with = "present")]
    pub ty_names: Option<Vec<String>>,
    #[serde(default)]
    pub currentness: Currentness,
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

// Seeds, then each step breadth first. A hop walks the view's edges once in
// append order and crosses every one that joins a frontier node to a node
// not yet found, so the earliest edge decides a node's provenance whatever
// order the seeds were given in. The frontier after a step is the nodes its
// last hop added. Edges are then the matching ones among the result nodes.
pub fn query<'a>(
    view: View<'a>,
    seeds: &[Seed],
    expand: &[Expansion],
    filter: &EdgeFilter,
) -> Result<ViewQuery, AppError> {
    if expand.iter().any(|step| step.hops == 0) {
        return Err(AppError::InvalidParams {
            message: "an expansion step needs at least one hop".into(),
        });
    }
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
    let mut hop = 0;
    for step in expand {
        let selection = EdgeSelection {
            ty_name: None,
            currentness: step.currentness,
        };
        for _ in 0..step.hops {
            if frontier.is_empty() {
                break;
            }
            hop += 1;
            let mut next = BTreeSet::new();
            for edge in view.select_edges(selection) {
                if !matches(step.ty_names.as_deref(), &edge.ty_name) {
                    continue;
                }
                let crossing = match step.direction {
                Direction::Outgoing => frontier
                    .contains(&edge.from)
                    .then_some((edge.from, edge.to)),
                Direction::Incoming => frontier.contains(&edge.to).then_some((edge.to, edge.from)),
                Direction::Both if frontier.contains(&edge.from) => Some((edge.from, edge.to)),
                Direction::Both if frontier.contains(&edge.to) => Some((edge.to, edge.from)),
                Direction::Both => None,
                };
                let Some((from, reached)) = crossing else {
                    continue;
                };
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
