use crate::{
    Entry, EntryId, View,
    analysis::{Config, SuiteConfig},
    index::CoreIndexes,
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

// Fields stay private to this module: entries and the suite change only
// through the staging methods below, which keep the derived indexes in step.
// Every `LifeCoreList` holds a valid history: loading validates it, and
// staging admits only entries that pass against it.
#[derive(Clone, Serialize)]
pub struct LifeCoreList {
    id: Uuid,
    entries: Vec<Entry>,
    suite: SuiteConfig,
    #[serde(skip)]
    indexes: CoreIndexes,
}

// The indexes are derived from `entries`, so they take no part in equality.
impl PartialEq for LifeCoreList {
    fn eq(&self, other: &Self) -> bool {
        let Self {
            id,
            entries,
            suite,
            indexes: _,
        } = self;
        *id == other.id && *entries == other.entries && *suite == other.suite
    }
}
impl Eq for LifeCoreList {}

// Debug prints the stored history only, as it did before the derived indexes.
impl std::fmt::Debug for LifeCoreList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Self {
            id,
            entries,
            suite,
            indexes: _,
        } = self;
        f.debug_struct("LifeCoreList")
            .field("id", id)
            .field("entries", entries)
            .field("suite", suite)
            .finish()
    }
}

// Deserialization is an import boundary. Invalid parents must never reach
// traversal.
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
            indexes: CoreIndexes::default(),
        };
        graph.indexes = graph.validate().map_err(serde::de::Error::custom)?;
        Ok(graph)
    }
}

impl Default for LifeCoreList {
    fn default() -> Self {
        Self {
            id: Uuid::new_v4(),
            entries: vec![],
            suite: SuiteConfig::default(),
            indexes: CoreIndexes::default(),
        }
    }
}

impl LifeCoreList {
    pub fn view(&self) -> View<'_> {
        View::new(self.id, &self.entries, Some(&self.indexes))
    }
    pub fn analysis_suite(&self) -> &SuiteConfig {
        &self.suite
    }
    // The first-child position of each entry, as the staleness index held it
    // before the core indexes generalized it.
    #[cfg(test)]
    pub(crate) fn staleness(&self) -> Vec<Option<usize>> {
        self.indexes.first_children()
    }
    // A history that has not been validated, for tests of validation itself.
    #[cfg(test)]
    pub(crate) fn unvalidated(id: Uuid, entries: Vec<Entry>) -> Self {
        Self {
            id,
            entries,
            suite: SuiteConfig::default(),
            indexes: CoreIndexes::default(),
        }
    }
    // Revalidates the whole history with freshly built indexes; the stored ones
    // are not consulted, but debug builds check that they still match.
    pub fn basic_validation(&self) -> Result<(), BasicValidationFailure> {
        let rebuilt = self.validate()?;
        debug_assert!(rebuilt == self.indexes, "stored indexes diverged");
        Ok(())
    }
    // Staging checks only the appended entry, against the indexed view of the
    // history before it, which is valid. That gives the result validating the
    // whole extended history would: every earlier entry passes, so the first
    // failure, if any, is this entry's. The entry is indexed only after it
    // passes, and a failure leaves the receiver unchanged.
    pub(crate) fn stage(&mut self, entry: Entry) -> Result<(), BasicValidationFailure> {
        let checked = self.check_next(&entry);
        #[cfg(debug_assertions)]
        self.assert_staged_like_full_validation(&entry, &checked);
        checked?;
        self.indexes.push(&entry);
        self.entries.push(entry);
        Ok(())
    }
    // Debug builds compare incremental validation with validating the extended
    // history from scratch: the same result, and on success the same indexes.
    #[cfg(debug_assertions)]
    fn assert_staged_like_full_validation(
        &mut self,
        entry: &Entry,
        checked: &Result<(), BasicValidationFailure>,
    ) {
        self.entries.push(entry.clone());
        let full = self.validate();
        self.entries.pop();
        match (full, checked) {
        (Ok(rebuilt), Ok(())) => {
            let mut extended = self.indexes.clone();
            extended.push(entry);
            assert!(rebuilt == extended, "extended indexes diverged");
        }
        (Err(full), Err(staged)) => assert_eq!(&full, staged, "staged validation diverged"),
        (full, staged) => panic!(
            "staged validation diverged: {staged:?} against {:?}",
            full.map(|_| ())
        ),
        }
    }
    // Staging appends one configuration to the suite; it adds no entry.
    pub(crate) fn stage_analyzer(&mut self, config: Config) {
        self.suite.0.push(config);
    }
    fn check_next(&self, entry: &Entry) -> Result<(), BasicValidationFailure> {
        if self.id.is_nil() {
            return Err(BasicValidationFailure::InvalidDatabase);
        }
        check_append(self.view(), entry)
    }
    // Validation and indexing share one pass: each entry is checked against the
    // prefix before it, whose indexes hold only entries that already passed.
    fn validate(&self) -> Result<CoreIndexes, BasicValidationFailure> {
        let mut indexes = CoreIndexes::default();
        self.validate_with(Some(&mut indexes))?;
        Ok(indexes)
    }
    // The reference: every check by its scan of the prefix before the entry.
    #[cfg(test)]
    pub(crate) fn validate_without_indexes(&self) -> Result<(), BasicValidationFailure> {
        self.validate_with(None)
    }
    #[cfg(test)]
    pub(crate) fn validate_with_indexes(&self) -> Result<(), BasicValidationFailure> {
        self.validate().map(drop)
    }
    fn validate_with(
        &self,
        mut indexes: Option<&mut CoreIndexes>,
    ) -> Result<(), BasicValidationFailure> {
        if self.id.is_nil() {
            return Err(BasicValidationFailure::InvalidDatabase);
        }
        for (position, entry) in self.entries.iter().enumerate() {
            let before = View::new(self.id, &self.entries[..position], indexes.as_deref());
            check_append(before, entry)?;
            if let Some(indexes) = indexes.as_deref_mut() {
                indexes.push(entry);
            }
        }
        Ok(())
    }
}

// Checks `entry` as the next append to `before`, which must be valid. Every
// reference and duplicate decision uses that prefix alone, through queries
// that read its indexes when it carries them and scan it otherwise, with the
// same results (SPEC §9, "Indexes").
fn check_append(before: View<'_>, entry: &Entry) -> Result<(), BasicValidationFailure> {
    let position = before.len();
    let id = entry.id();
    if id.position != position || id.origin.is_nil() {
        return Err(BasicValidationFailure::InvalidIdentity { position });
    }
    let require_node = |target| {
        before
            .node(target)
            .ok_or(BasicValidationFailure::InvalidReference { entry: id, target })
    };
    match entry {
    Entry::Node(n) => {
        if let Some(predecessor) = n.predecessor {
            require_node(predecessor)?;
        }
        if before
            .nodes_with_content(&n.ty_name, n.predecessor, &n.payload)
            .any(|old| before.is_stale(old.id) == Some(false))
        {
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
        if before.edge_between(e.from, e.to, &e.ty_name).is_some() {
            return Err(BasicValidationFailure::Duplicate { entry: id });
        }
    }
    }
    Ok(())
}
