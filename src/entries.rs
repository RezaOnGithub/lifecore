use crate::Body;
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

// Origin distinguishes unrelated databases and independently prepared draft
// suffixes. A committed entry retains its original handle across snapshots
// and reloads.
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

// Entries serialize under their field names, the proposal vocabulary of FILES
// 2.5, tagged by `kind` within `Entry`; a root omits `predecessor`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: EntryId,
    pub ty_name: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub predecessor: Option<EntryId>,
    pub payload: Body,
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
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Entry {
    Node(Node),
    Edge(Edge),
}

// An optional field is either omitted or valid; JSON null is not a second
// spelling of absence.
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
