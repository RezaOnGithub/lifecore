use crate::{Entry, EntryId, View, analysis::SuiteConfig};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum BasicValidationFailure {
    #[error("entry identity does not match position {position}")]
    InvalidIdentity { position: usize },
    #[error("entry {entry:?} references an unavailable or non-node entry {target:?}")]
    InvalidReference { entry: EntryId, target: EntryId },
    #[error("entry {entry:?} duplicates an eligible earlier entry")]
    Duplicate { entry: EntryId },
    #[error("edge {entry:?} references a stale endpoint")]
    StaleEndpoint { entry: EntryId },
    #[error("edge {entry:?} connects a node to itself")]
    SelfEdge { entry: EntryId },
    #[error("database identity is missing")]
    InvalidDatabase,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LifeCoreList {
    id: Uuid,
    pub(crate) entries: Vec<Entry>,
    pub(crate) suite: SuiteConfig,
}

//A Deserialization is an import boundary. Invalid parents must never reach
//A traversal.
impl<'de> Deserialize<'de> for LifeCoreList {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Stored {
            id: Uuid,
            entries: Vec<Entry>,
            suite: SuiteConfig,
        }
        let stored = Stored::deserialize(deserializer)?;
        let graph = Self {
            id: stored.id,
            entries: stored.entries,
            suite: stored.suite,
        };
        graph.basic_validation().map_err(serde::de::Error::custom)?;
        Ok(graph)
    }
}

impl Default for LifeCoreList {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            entries: vec![],
            suite: SuiteConfig::default(),
        }
    }
}

impl LifeCoreList {
    pub fn view(&self) -> View<'_> {
        View::new(self.id, &self.entries)
    }
    pub fn analysis_suite(&self) -> &SuiteConfig {
        &self.suite
    }
    pub fn basic_validation(&self) -> Result<(), BasicValidationFailure> {
        if self.id.is_nil() {
            return Err(BasicValidationFailure::InvalidDatabase);
        }
        for (position, entry) in self.entries.iter().enumerate() {
            let id = entry.id();
            if id.position != position || id.origin.is_nil() {
                return Err(BasicValidationFailure::InvalidIdentity { position });
            }
            //A Every reference and duplicate decision uses the prefix immediately before
            //A this entry.
            let before = View::new(self.id, &self.entries[..position]);
            let require_node = |target| {
                before
                    .node(target)
                    .ok_or(BasicValidationFailure::InvalidReference { entry: id, target })
            };
            match entry {
            Entry::Node(n) => {
                if let Some(parent) = n.parent {
                    require_node(parent)?;
                }
                if before.fresh_nodes().any(|old| {
                    old.ty_name == n.ty_name && old.parent == n.parent && old.body == n.body
                }) {
                    return Err(BasicValidationFailure::Duplicate { entry: id });
                }
            }
            Entry::Edge(e) => {
                require_node(e.from)?;
                require_node(e.to)?;
                if e.from == e.to {
                    return Err(BasicValidationFailure::SelfEdge { entry: id });
                }
                if before.is_stale(e.from) == Some(true) || before.is_stale(e.to) == Some(true) {
                    return Err(BasicValidationFailure::StaleEndpoint { entry: id });
                }
                if before
                    .edges()
                    .any(|old| old.from == e.from && old.to == e.to && old.ty_name == e.ty_name)
                {
                    return Err(BasicValidationFailure::Duplicate { entry: id });
                }
            }
            }
        }
        Ok(())
    }
}
