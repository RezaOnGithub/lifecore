use crate::Body;
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

//A Origin distinguishes unrelated databases and independently prepared draft
//A suffixes. A committed entry retains its original handle across snapshots
//A and reloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryId {
    #[serde(with = "crate::decimal")]
    pub(crate) position: usize,
    pub(crate) origin: Uuid,
}

impl EntryId {
    pub fn position(self) -> usize {
        self.position
    }
}

//A Entries serialize in the proposal vocabulary (FILES 2.5), so the snapshot,
//A analysis output, RPC results, and change files share one language:
//A `type`, `predecessor` (omitted for a root), and `payload`, tagged by `kind`
//A within `Entry`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: EntryId,
    #[serde(rename = "type")]
    pub ty_name: String,
    #[serde(
        rename = "predecessor",
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub parent: Option<EntryId>,
    #[serde(rename = "payload")]
    pub body: Body,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edge {
    pub id: EntryId,
    #[serde(rename = "type")]
    pub ty_name: String,
    pub from: EntryId,
    pub to: EntryId,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Entry {
    Node(Node),
    Edge(Edge),
}

//A An optional field is either omitted or valid; JSON null is not a second
//A spelling of absence.
pub(crate) fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
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
