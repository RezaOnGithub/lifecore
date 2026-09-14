use super::{FindingKey, Invocation, MessageType, Observation, ObservationScope, Report};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fmt::Write,
    hash::{BuildHasher, RandomState},
};
use std::{
    fs::{File, OpenOptions},
    io::Write as IoWrite,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dismissal {
    pub key: FindingKey,
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dismissals {
    records: Vec<Dismissal>,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("inconsistencies cannot be dismissed")]
pub struct CannotDismissInconsistency;

impl Dismissals {
    pub fn records(&self) -> impl Iterator<Item = &Dismissal> {
        self.records.iter()
    }
    pub fn get(&self, key: &FindingKey) -> Option<&Dismissal> {
        self.records.iter().find(|record| record.key == *key)
    }
    pub fn dismiss(
        &mut self,
        key: FindingKey,
        reason: String,
    ) -> Result<(), CannotDismissInconsistency> {
        if key.kind == MessageType::Inconsistency {
            return Err(CannotDismissInconsistency);
        }
        if let Some(record) = self.records.iter_mut().find(|record| record.key == key) {
            record.reason = reason;
        } else {
            self.records.push(Dismissal { key, reason });
        }
        Ok(())
    }
    //A Restore is keyed by the stored record, without requiring an emitted finding.
    pub fn restore(&mut self, key: &FindingKey) -> bool {
        let before = self.records.len();
        self.records.retain(|record| record.key != *key);
        before != self.records.len()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Encoding(#[from] serde_json::Error),
    #[error(transparent)]
    Inconsistency(#[from] CannotDismissInconsistency),
    #[error("dismissals were published but directory synchronization failed: {0}")]
    Published(std::io::Error),
}

pub struct DismissalStore {
    path: PathBuf,
}

impl DismissalStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
    pub fn load(&self) -> Result<Dismissals, StoreError> {
        match File::open(&self.path) {
        Ok(file) => Ok(serde_json::from_reader(file)?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Dismissals::default()),
        Err(error) => Err(error.into()),
        }
    }
    pub fn dismiss(&self, key: FindingKey, reason: String) -> Result<(), StoreError> {
        let _lock = self.lock()?;
        let mut state = self.load()?;
        state.dismiss(key, reason)?;
        self.save(&state)
    }
    pub fn restore(&self, key: &FindingKey) -> Result<bool, StoreError> {
        let _lock = self.lock()?;
        let mut state = self.load()?;
        let restored = state.restore(key);
        if restored {
            self.save(&state)?;
        }
        Ok(restored)
    }
    fn lock(&self) -> Result<File, StoreError> {
        //A Dismissal edits reread under writer exclusion, independent of graph
        //A publication. Two frontends must not overwrite each other's records.
        let mut name = self.path.as_os_str().to_os_string();
        name.push(".lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(name)?;
        file.lock()?;
        Ok(file)
    }
    fn save(&self, state: &Dismissals) -> Result<(), StoreError> {
        let directory = self
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut temporary = tempfile::NamedTempFile::new_in(directory)?;
        serde_json::to_writer_pretty(temporary.as_file_mut(), state)?;
        temporary.write_all(b"\n")?;
        temporary.as_file().sync_all()?;
        temporary.persist(&self.path).map_err(|e| e.error)?;
        File::open(directory)
            .and_then(|directory| directory.sync_all())
            .map_err(StoreError::Published)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ReportSelection {
    #[default]
    Current,
    History,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AdvisoryFilter {
    #[default]
    All,
    Warnings,
    Suggestions,
    None,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ReportOptions {
    pub selection: ReportSelection,
    pub advisories: AdvisoryFilter,
    pub include_dismissed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct FindingGroup<'a> {
    pub key: &'a FindingKey,
    pub observations: Vec<&'a Observation>,
    pub selected: Vec<&'a Observation>,
    pub dismissal: Option<&'a Dismissal>,
}

impl Report {
    pub fn groups<'a>(
        &'a self,
        dismissals: &'a Dismissals,
        options: ReportOptions,
    ) -> Vec<FindingGroup<'a>> {
        self.groups_with_hasher(dismissals, options, RandomState::new())
    }

    pub(super) fn groups_with_hasher<'a, S: BuildHasher>(
        &'a self,
        dismissals: &'a Dismissals,
        options: ReportOptions,
        hasher: S,
    ) -> Vec<FindingGroup<'a>> {
        //A The map is only an index; output follows first observation order.
        //A HashMap compares complete keys even if hashes collide.
        let mut index = HashMap::<&FindingKey, usize, S>::with_hasher(hasher);
        let mut groups = Vec::<FindingGroup<'a>>::new();
        for observation in self.observations() {
            let key = &observation.key;
            let position = *index.entry(key).or_insert_with(|| {
                groups.push(FindingGroup {
                    key,
                    observations: vec![],
                    selected: vec![],
                    dismissal: dismissals.get(key),
                });
                groups.len() - 1
            });
            let group = &mut groups[position];
            group.observations.push(observation);
            let inconsistency = observation.diagnostic.ty() == MessageType::Inconsistency;
            let category = match options.advisories {
            AdvisoryFilter::All => true,
            AdvisoryFilter::Warnings => observation.diagnostic.ty() == MessageType::Warning,
            AdvisoryFilter::Suggestions => observation.diagnostic.ty() == MessageType::Suggestion,
            AdvisoryFilter::None => false,
            };
            let boundary = options.selection == ReportSelection::History
                || observation.invocation == Invocation::Initialization
                || observation.invocation == Invocation::Prefix(self.view)
                || observation.diagnostic.scope == ObservationScope::Event;
            if inconsistency
                || (category
                    && boundary
                    && (options.include_dismissed || group.dismissal.is_none()))
            {
                group.selected.push(observation);
            }
        }
        groups.retain(|group| !group.selected.is_empty());
        groups
    }

    pub fn render(&self, dismissals: &Dismissals, options: ReportOptions) -> String {
        let mut text = format!(
            "{:?} report at boundary {} ({})\n",
            options.selection, self.view.end, self.view.database
        );
        for group in self.groups(dismissals, options) {
            writeln!(
                text,
                "{:?} {} [{}]{}",
                group.key.kind,
                group.key.finding.code,
                group.key.digest(),
                if group.dismissal.is_some() && group.key.kind != MessageType::Inconsistency {
                    " (dismissed)"
                } else {
                    ""
                }
            )
            .unwrap();
            for observation in group.selected {
                let at = match observation.invocation {
                Invocation::Initialization => "initialization".to_string(),
                Invocation::Prefix(view) => format!("boundary {}", view.end),
                };
                let retained = if observation.diagnostic.scope == ObservationScope::Event
                    && observation.invocation != Invocation::Prefix(self.view)
                {
                    " (historical event; current applicability unconfirmed)"
                } else {
                    ""
                };
                writeln!(text, "  {at}{retained}: {}", observation.diagnostic.message).unwrap();
            }
        }
        text
    }
}
