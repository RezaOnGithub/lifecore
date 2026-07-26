use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::File,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
};

use lifecore::{LifeCore, LifeCoreList};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tempfile::NamedTempFile;
use uuid::{Uuid, Version};

use crate::error::AppError;

pub const STATE_FORMAT_VERSION: u32 = 1;
pub const ENTRY_FINGERPRINT_VERSION: u32 = 1;
pub const MESSAGE_ID_VERSION: u32 = 1;
pub const DEFAULT_STATE_FILE_NAME: &str = "LifeCoreState.json";
pub const STATE_PATH_ENVIRONMENT: &str = "LIFECORE_STATE_FILE_PATH";

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredEntryLocatorV1 {
    pub version: u32,
    pub index: u64,
    pub digest: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DismissalRecord {
    pub version: u32,
    pub digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliStateV1 {
    pub version: u32,
    pub uuid: Uuid,
    pub entry_fingerprint_version: u32,
    pub message_id_version: u32,
    pub aliases: BTreeMap<String, StoredEntryLocatorV1>,
    pub dismissals: Vec<DismissalRecord>,
    pub sidecar_path: Option<PathBuf>,
    pub reserved: BTreeMap<String, Value>,
}

impl CliStateV1 {
    pub fn new() -> Self {
        Self {
            version: STATE_FORMAT_VERSION,
            uuid: Uuid::now_v7(),
            entry_fingerprint_version: ENTRY_FINGERPRINT_VERSION,
            message_id_version: MESSAGE_ID_VERSION,
            aliases: BTreeMap::new(),
            dismissals: Vec::new(),
            sidecar_path: None,
            reserved: BTreeMap::new(),
        }
    }

    fn validate(&self) -> Result<(), AppError> {
        if self.version != STATE_FORMAT_VERSION {
            return Err(AppError::invalid_state(format!(
                "unsupported CLI state version {}; expected {STATE_FORMAT_VERSION}",
                self.version
            )));
        }

        if self.uuid.get_version() != Some(Version::SortRand) {
            return Err(AppError::invalid_state("state UUID is not UUIDv7"));
        }

        if self.entry_fingerprint_version != ENTRY_FINGERPRINT_VERSION {
            return Err(AppError::invalid_state(format!(
                "unsupported entry-fingerprint version {}; expected {ENTRY_FINGERPRINT_VERSION}",
                self.entry_fingerprint_version
            )));
        }

        if self.message_id_version != MESSAGE_ID_VERSION {
            return Err(AppError::invalid_state(format!(
                "unsupported message-ID version {}; expected {MESSAGE_ID_VERSION}",
                self.message_id_version
            )));
        }

        for (name, locator) in &self.aliases {
            crate::reference::validate_alias(name).map_err(|error| {
                AppError::invalid_state(format!("invalid alias {name:?}: {error}"))
            })?;

            if locator.version != ENTRY_FINGERPRINT_VERSION {
                return Err(AppError::invalid_state(format!(
                    "alias {name:?} has unsupported entry-fingerprint version {}",
                    locator.version
                )));
            }

            validate_digest(&locator.digest, "alias entry fingerprint")?;
        }

        let mut previous = None;
        for dismissal in &self.dismissals {
            if dismissal.version != MESSAGE_ID_VERSION {
                return Err(AppError::invalid_state(format!(
                    "dismissal has unsupported message-ID version {}",
                    dismissal.version
                )));
            }

            validate_digest(&dismissal.digest, "dismissal message ID")?;

            if previous
                .as_ref()
                .is_some_and(|value| value >= &dismissal.digest)
            {
                return Err(AppError::invalid_state(
                    "dismissal registry must be strictly sorted and contain no duplicates",
                ));
            }

            previous = Some(dismissal.digest.clone());
        }

        Ok(())
    }
}

impl Default for CliStateV1 {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistedState {
    pub lifecore: LifeCoreList,
    pub cli: CliStateV1,
}

impl PersistedState {
    pub fn new(lifecore: LifeCoreList) -> Self {
        Self {
            lifecore,
            cli: CliStateV1::new(),
        }
    }

    pub fn validate(&self) -> Result<(), AppError> {
        self.cli.validate()?;
        self.lifecore.basic_validation().map_err(|error| {
            AppError::invalid_state(format!("basic validation failed: {error:?}"))
        })?;

        if self.lifecore.is_mock() {
            return Err(AppError::invalid_state(
                "persisted graph contains a Mock circumstance",
            ));
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StatePath(PathBuf);

impl StatePath {
    pub fn resolve(
        environment_value: Option<OsString>,
        current_directory: &Path,
    ) -> Result<Self, AppError> {
        let selected = environment_value
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_STATE_FILE_NAME));

        if selected.as_os_str().is_empty() {
            return Err(AppError::invalid_state_path(format!(
                "{STATE_PATH_ENVIRONMENT} names an empty path"
            )));
        }

        let path = if selected.is_absolute() {
            selected
        } else {
            current_directory.join(selected)
        };

        Ok(Self(path))
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }
}

#[derive(Clone, Debug)]
pub struct StateStore {
    path: StatePath,
    #[cfg(test)]
    fail_save: bool,
}

impl StateStore {
    pub fn new(path: StatePath) -> Self {
        Self {
            path,
            #[cfg(test)]
            fail_save: false,
        }
    }

    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    pub fn load(&self) -> Result<PersistedState, AppError> {
        log::trace!("loading canonical state from {}", self.path().display());

        let file =
            File::open(self.path()).map_err(|error| AppError::state_io(self.path(), error))?;
        let state: PersistedState = serde_json::from_reader(BufReader::new(file))
            .map_err(|error| AppError::state_decode(self.path(), error))?;

        state.validate()?;

        Ok(state)
    }

    pub fn initialize(&self, state: &PersistedState) -> Result<(), AppError> {
        state.validate()?;
        let temporary = self.write_temporary(state)?;

        log::trace!(
            "preparing no-clobber state initialization at {}",
            self.path().display()
        );

        temporary
            .persist_noclobber(self.path())
            .map_err(|error| AppError::state_io(self.path(), error.error))?;

        Ok(())
    }

    pub fn save(&self, state: &PersistedState) -> Result<(), AppError> {
        state.validate()?;

        #[cfg(test)]
        if self.fail_save {
            //A The transaction tests need a deterministic failure after every
            //A graph gate but before replacement; filesystem permissions are
            //A unreliable when the test process has elevated privileges.
            return Err(AppError::state_io(
                self.path(),
                std::io::Error::other("injected atomic-save failure"),
            ));
        }

        let temporary = self.write_temporary(state)?;

        log::trace!("atomically replacing state at {}", self.path().display());

        temporary
            .persist(self.path())
            .map_err(|error| AppError::state_io(self.path(), error.error))?;

        Ok(())
    }

    fn write_temporary(&self, state: &PersistedState) -> Result<NamedTempFile, AppError> {
        let parent = self
            .path()
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut temporary =
            NamedTempFile::new_in(parent).map_err(|error| AppError::state_io(parent, error))?;

        {
            let mut writer = BufWriter::new(temporary.as_file_mut());
            serde_json::to_writer_pretty(&mut writer, state)
                .map_err(|error| AppError::state_encode(self.path(), error))?;
            writer
                .write_all(b"\n")
                .map_err(|error| AppError::state_io(self.path(), error))?;
            writer
                .flush()
                .map_err(|error| AppError::state_io(self.path(), error))?;
        }

        temporary
            .as_file()
            .sync_all()
            .map_err(|error| AppError::state_io(self.path(), error))?;

        Ok(temporary)
    }

    #[cfg(test)]
    pub(crate) fn failing_save_for_test(&self) -> Self {
        Self {
            path: self.path.clone(),
            fail_save: true,
        }
    }
}

fn validate_digest(digest: &str, field: &str) -> Result<(), AppError> {
    if digest.len() != 128
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(AppError::invalid_state(format!(
            "{field} must contain exactly 128 lowercase hexadecimal digits"
        )));
    }

    Ok(())
}
