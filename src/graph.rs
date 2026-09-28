use crate::{
    Entry, EntryId, View,
    analysis::{Config, SuiteConfig},
};
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

//A Fields stay private to this module: entries and the suite change only
//A through the staging methods below, which keep the derived index in step.
#[derive(Clone, Debug, Serialize)]
pub struct LifeCoreList {
    id: Uuid,
    entries: Vec<Entry>,
    suite: SuiteConfig,
    #[serde(skip)]
    staleness: StalenessIndex,
}

//A The staleness index is derived from `entries`, so it takes no part in equality.
impl PartialEq for LifeCoreList {
    fn eq(&self, other: &Self) -> bool {
        let Self {
            id,
            entries,
            suite,
            staleness: _,
        } = self;
        *id == other.id && *entries == other.entries && *suite == other.suite
    }
}
impl Eq for LifeCoreList {}

//A Derived and never serialized: one slot per entry, holding the position of that
//A node's first child. History is append-only, so a slot is set at most once and
//A one index serves every prefix view and candidate clone; `View` compares each
//A value with its own length. Only entries already validated against the prefix
//A before them are indexed, so an invalid parent never marks an earlier node stale.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct StalenessIndex(Vec<Option<usize>>);

impl StalenessIndex {
    fn as_slice(&self) -> &[Option<usize>] {
        &self.0
    }
    fn push_validated(&mut self, entry: &Entry) {
        let position = self.0.len();
        if let Some(parent) = entry.as_node().and_then(|node| node.parent) {
            let first_child = &mut self.0[parent.position];
            if first_child.is_none() {
                *first_child = Some(position);
            }
        }
        self.0.push(None);
    }
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
        let mut graph = Self {
            id: stored.id,
            entries: stored.entries,
            suite: stored.suite,
            staleness: StalenessIndex::default(),
        };
        graph.staleness = graph.validate().map_err(serde::de::Error::custom)?;
        Ok(graph)
    }
}

impl Default for LifeCoreList {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            entries: vec![],
            suite: SuiteConfig::default(),
            staleness: StalenessIndex::default(),
        }
    }
}

impl LifeCoreList {
    pub fn view(&self) -> View<'_> {
        View::new(self.id, &self.entries, self.staleness.as_slice())
    }
    pub fn analysis_suite(&self) -> &SuiteConfig {
        &self.suite
    }
    #[cfg(test)]
    pub(crate) fn staleness(&self) -> &[Option<usize>] {
        self.staleness.as_slice()
    }
    //A Revalidates the whole history with a freshly built index; the stored one is
    //A not consulted, but debug builds check that it still matches.
    pub fn basic_validation(&self) -> Result<(), BasicValidationFailure> {
        let rebuilt = self.validate()?;
        debug_assert_eq!(rebuilt, self.staleness, "stored staleness index diverged");
        Ok(())
    }
    //A Staging appends one entry, validates the resulting history, and indexes the
    //A entry only after it passes. A failure leaves the receiver unchanged.
    pub(crate) fn stage(&mut self, entry: Entry) -> Result<(), BasicValidationFailure> {
        self.entries.push(entry);
        let rebuilt = match self.validate() {
        Ok(rebuilt) => rebuilt,
        Err(failure) => {
            self.entries.pop();
            return Err(failure);
        }
        };
        self.staleness
            .push_validated(self.entries.last().expect("staged entry"));
        debug_assert_eq!(rebuilt, self.staleness, "extended staleness index diverged");
        Ok(())
    }
    //A Staging appends one configuration to the suite; it adds no entry.
    pub(crate) fn stage_analyzer(&mut self, config: Config) {
        self.suite.0.push(config);
    }
    //A Validation and indexing share one pass: each entry is checked against the
    //A prefix before it, whose index holds only entries that already passed.
    fn validate(&self) -> Result<StalenessIndex, BasicValidationFailure> {
        if self.id.is_nil() {
            return Err(BasicValidationFailure::InvalidDatabase);
        }
        let mut staleness = StalenessIndex::default();
        for (position, entry) in self.entries.iter().enumerate() {
            let id = entry.id();
            if id.position != position || id.origin.is_nil() {
                return Err(BasicValidationFailure::InvalidIdentity { position });
            }
            //A Every reference and duplicate decision uses the prefix immediately before
            //A this entry.
            let before = View::new(self.id, &self.entries[..position], staleness.as_slice());
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
            staleness.push_validated(entry);
        }
        Ok(staleness)
    }
}
