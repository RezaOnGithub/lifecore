use crate::Body;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

//A Origin distinguishes unrelated databases and independently prepared draft
//A suffixes. A committed entry retains its original handle across snapshots
//A and reloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryId {
    pub(crate) position: usize,
    pub(crate) origin: Uuid,
}

impl EntryId {
    pub fn position(self) -> usize {
        self.position
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: EntryId,
    pub ty_name: String,
    pub body: Body,
    pub parent: Option<EntryId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edge {
    pub id: EntryId,
    pub ty_name: String,
    pub from: EntryId,
    pub to: EntryId,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum Entry {
    Node(Node),
    Edge(Edge),
}

impl Entry {
    pub fn id(&self) -> EntryId {
        match self {
        Self::Node(n) => n.id,
        Self::Edge(e) => e.id,
        }
    }
    pub fn as_node(&self) -> Option<&Node> {
        match self {
        Self::Node(n) => Some(n),
        Self::Edge(_) => None,
        }
    }
    pub fn as_edge(&self) -> Option<&Edge> {
        match self {
        Self::Edge(e) => Some(e),
        Self::Node(_) => None,
        }
    }
}
