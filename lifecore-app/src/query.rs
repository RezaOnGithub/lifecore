use crate::{AppError, proposal::present, service::EntryRecord};
use lifecore::{
    Edge, Entry, EntryId, Node, View, ViewId,
    inspection::{Currentness, Direction, EdgeSelection, NodeSelection},
    typing::TypeResolution,
};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use std::{
    collections::{BTreeMap, BTreeSet},
    iter,
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
    pub other: EntryId,
}

// A node with its type, its whole revision tree in append order (root first,
// itself included), every incident edge of any currentness in append order,
// and the record of each edge's other endpoint once, in append order, however
// many edges reach it; or an edge with its endpoints.
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
        neighbours: Vec<EntryRecord>,
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
        let root = view
            .root_of(id)
            .and_then(|root| view.node(root))
            .expect("a node of the view has its root there");
        let lineage = iter::once(root)
            .chain(view.descendants(root.id))
            .map(|member| EntryRecord::node(view, member))
            .collect();
        let mut edges = vec![];
        // Keyed by position, which is append order within one view.
        let mut neighbours = BTreeMap::new();
        for edge in view.edges_of(id) {
            let (direction, other) = if edge.from == id {
                (Incidence::Outgoing, edge.to)
            } else {
                (Incidence::Incoming, edge.from)
            };
            neighbours.insert(other.position(), endpoint(view, other)?);
            edges.push(IncidentEdge {
                edge: EntryRecord::edge(view, edge),
                direction,
                other,
            });
        }
        EntryInspection::Node {
            view: view.id(),
            entry: EntryRecord::node(view, node),
            ty: view.resolve_type(&node.ty_name),
            lineage,
            edges,
            neighbours: neighbours
                .into_values()
                .map(|neighbour| EntryRecord::node(view, neighbour))
                .collect(),
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

// Why an `ids` seed names no node of the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeedRefusal {
    Absent,
    Edge,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefusedSeed {
    pub id: EntryId,
    pub reason: SeedRefusal,
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

// Seeds, all refused together before any expansion if some `ids` seed is
// not a node of the view, then each step breadth first. A hop reads the
// frontier's incident edges and crosses, in append order, every matching one
// that joins a frontier node to a node not yet found, so the earliest edge
// decides a node's provenance whatever order the seeds were given in. The
// frontier after a step is the nodes its last hop added. Edges are then the
// matching ones among the result nodes.
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
    // In request order, each id once.
    let mut refused = vec![];
    let mut refused_ids = BTreeSet::new();
    for selection in seeds {
        match selection {
        Seed::Ids { ids } => {
            for &id in ids {
                let reason = match view.get(id) {
                Some(Entry::Node(node)) => {
                    seed(node, &mut frontier);
                    continue;
                }
                Some(Entry::Edge(_)) => SeedRefusal::Edge,
                None => SeedRefusal::Absent,
                };
                if refused_ids.insert(id) {
                    refused.push(RefusedSeed { id, reason });
                }
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
    if !refused.is_empty() {
        return Err(AppError::InvalidSeeds { refused });
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
