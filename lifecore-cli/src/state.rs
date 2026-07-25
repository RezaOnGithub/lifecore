use std::{
    collections::BTreeMap,
    env,
    fs::{self, File, OpenOptions},
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::{Context as _, Result, bail};
use lifecore::{LifeCore, LifeCoreList};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::locator::DurableLocator;

pub const DEFAULT_STATE_FILE: &str = "LifeCoreState.json";
pub const STATE_FILE_ENV: &str = "LIFECORE_STATE_FILE_PATH";
pub const STATE_FORMAT_VERSION: u16 = 1;
pub const ENTRY_FINGERPRINT_VERSION: u16 = 1;
pub const MESSAGE_ID_VERSION: u16 = 1;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dismissal {
    pub version: u16,
    pub digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliState {
    pub state_format_version: u16,
    pub state_uuid: Uuid,
    pub entry_fingerprint_version: u16,
    pub message_id_version: u16,
    pub aliases: BTreeMap<String, DurableLocator>,
    pub dismissals: Vec<Dismissal>,
    pub sidecar: Option<PathBuf>,
    pub metadata: BTreeMap<String, serde_json::Value>,
}

impl CliState {
    pub fn new() -> Self {
        Self {
            state_format_version: STATE_FORMAT_VERSION,
            state_uuid: Uuid::now_v7(),
            entry_fingerprint_version: ENTRY_FINGERPRINT_VERSION,
            message_id_version: MESSAGE_ID_VERSION,
            aliases: BTreeMap::new(),
            dismissals: Vec::new(),
            sidecar: None,
            metadata: BTreeMap::new(),
        }
    }

    fn validate(&self) -> Result<()> {
        if self.state_format_version != STATE_FORMAT_VERSION {
            bail!(
                "unsupported CLI state format version {}; expected {}",
                self.state_format_version,
                STATE_FORMAT_VERSION
            );
        }
        if self.entry_fingerprint_version != ENTRY_FINGERPRINT_VERSION {
            bail!("unsupported entry fingerprint version");
        }
        if self.message_id_version != MESSAGE_ID_VERSION {
            bail!("unsupported message ID version");
        }
        if self.aliases.contains_key("HEAD") {
            bail!("state contains reserved alias \"HEAD\"");
        }
        Ok(())
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct StateFile {
    pub lifecore: LifeCoreList,
    pub cli: CliState,
}

impl StateFile {
    pub fn new() -> Self {
        Self {
            lifecore: LifeCoreList::default(),
            cli: CliState::new(),
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let file = File::open(path)
            .with_context(|| format!("failed to open state file {}", path.display()))?;
        let state: Self = serde_json::from_reader(BufReader::new(file)).with_context(|| {
            format!(
                "failed to deserialize version-1 state file {}; older CLI states are unsupported",
                path.display()
            )
        })?;
        state.cli.validate()?;
        if state.lifecore.is_mock() {
            bail!("persisted graph contains a provisional Mock circumstance");
        }
        for (name, locator) in &state.cli.aliases {
            crate::locator::validate_alias(name)?;
            crate::locator::resolve_node(&state, locator, latest_view(&state.lifecore))
                .with_context(|| format!("invalid durable alias {name:?}"))?;
        }
        Ok(state)
    }

    pub fn create(&self, path: &Path) -> Result<()> {
        self.write(path, true)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        self.write(path, false)
    }

    fn write(&self, path: &Path, create_only: bool) -> Result<()> {
        self.cli.validate()?;
        if self.lifecore.is_mock() {
            bail!("refusing to persist a graph containing Mock");
        }
        if create_only && path.exists() {
            bail!("state file {} already exists", path.display());
        }
        let parent = path
            .parent()
            .filter(|value| !value.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let name = path
            .file_name()
            .context("state file path has no file name")?
            .to_string_lossy();
        let temporary = parent.join(format!(
            ".{name}.{}.{}.tmp",
            process::id(),
            TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));

        let result = (|| -> Result<()> {
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            let mut writer = BufWriter::new(file);
            serde_json::to_writer_pretty(&mut writer, self)?;
            writer.write_all(b"\n")?;
            writer.flush()?;
            writer.get_ref().sync_all()?;
            if create_only {
                fs::hard_link(&temporary, path)
                    .with_context(|| format!("state file {} already exists", path.display()))?;
                fs::remove_file(&temporary)?;
            } else {
                fs::rename(&temporary, path)?;
            }
            if let Ok(directory) = File::open(parent) {
                drop(directory.sync_all());
            }
            Ok(())
        })();
        if result.is_err() {
            drop(fs::remove_file(&temporary));
        }
        result
    }
}

pub fn state_path() -> Result<PathBuf> {
    match env::var_os(STATE_FILE_ENV) {
    Some(value) if value.is_empty() => bail!("{STATE_FILE_ENV} is defined but empty"),
    Some(value) => Ok(PathBuf::from(value)),
    None => Ok(env::current_dir()?.join(DEFAULT_STATE_FILE)),
    }
}

pub fn absolute_existing_path(path: &Path) -> Result<PathBuf> {
    if !path.exists() {
        bail!("sidecar file {} does not exist", path.display());
    }
    Ok(if path.is_absolute() {
        path.to_path_buf()
    } else {
        env::current_dir()?.join(path)
    })
}

pub fn latest_view(lifecore: &LifeCoreList) -> lifecore::Context {
    lifecore
        .last()
        .map_or_else(|| lifecore::EntryIndex(0).before(), |last| last.window())
}
