use std::{
    collections::BTreeMap,
    env,
    fs::{self, File, OpenOptions},
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
};

use anyhow::{Context, Result, anyhow, bail};
use lifecore::{LifeCore, LifeCoreList, PointedEventContext};
use serde::{Deserialize, Serialize};

pub const DEFAULT_STATE_FILE: &str = "LifeCoreState.json";
pub const STATE_FILE_ENV: &str = "LIFECORE_STATE_FILE_PATH";

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Barklist {
    pub hashes: Vec<String>,
    pub resolved: Vec<PointedEventContext>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CliState {
    pub aliases: BTreeMap<String, PointedEventContext>,
    pub barklist: Barklist,
    pub sidecar: Option<PathBuf>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct StateFile {
    pub lifecore: LifeCoreList,
    pub cli: CliState,
}

impl StateFile {
    pub fn empty() -> Self {
        Self {
            lifecore: LifeCoreList::default(),
            cli: CliState::default(),
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        log::debug!("loading state from {}", path.display());
        let file = File::open(path)
            .with_context(|| format!("failed to open state file {}", path.display()))?;
        let state: Self = serde_json::from_reader(BufReader::new(file))
            .with_context(|| format!("failed to deserialize state file {}", path.display()))?;
        if state.lifecore.is_mock() {
            bail!(
                "state file {} contains a provisional Mock circumstance and cannot be used",
                path.display()
            );
        }
        for (name, context) in &state.cli.aliases {
            if !context.validate() || state.lifecore.node(*context).is_none() {
                bail!(
                    "state file {} contains invalid node alias {name:?}",
                    path.display()
                );
            }
        }
        Ok(state)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        self.write(path, false)
    }

    pub fn create(&self, path: &Path) -> Result<()> {
        self.write(path, true)
    }

    fn write(&self, path: &Path, create_only: bool) -> Result<()> {
        log::debug!(
            "persisting state to {} (create_only={create_only})",
            path.display()
        );
        if self.lifecore.is_mock() {
            bail!("refusing to persist a database containing Mock circumstances");
        }
        if create_only && path.exists() {
            bail!("state file {} already exists", path.display());
        }

        let parent = path
            .parent()
            .filter(|x| !x.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let file_name = path
            .file_name()
            .ok_or_else(|| anyhow!("state file path {} has no file name", path.display()))?;
        let counter = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".{}.{}.{}.tmp",
            file_name.to_string_lossy(),
            process::id(),
            counter
        ));

        let result = (|| -> Result<()> {
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .with_context(|| {
                    format!(
                        "failed to create temporary state file {}",
                        temporary.display()
                    )
                })?;
            let mut writer = BufWriter::new(file);
            serde_json::to_writer_pretty(&mut writer, self).context("failed to serialize state")?;
            writer
                .write_all(b"\n")
                .context("failed to terminate state JSON")?;
            writer.flush().context("failed to flush state data")?;
            writer
                .get_ref()
                .sync_all()
                .context("failed to synchronize state data")?;

            if create_only && path.exists() {
                bail!("state file {} already exists", path.display());
            }
            fs::rename(&temporary, path).with_context(|| {
                format!(
                    "failed to atomically replace state file {} using {}",
                    path.display(),
                    temporary.display()
                )
            })?;
            log::trace!(
                "renamed temporary state {} to {}",
                temporary.display(),
                path.display()
            );
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
    Some(path) if path.is_empty() => bail!("{STATE_FILE_ENV} is defined but empty"),
    Some(path) => Ok(PathBuf::from(path)),
    None => Ok(env::current_dir()
        .context("failed to inspect the current directory")?
        .join(DEFAULT_STATE_FILE)),
    }
}

pub fn absolute_existing_path(path: &Path) -> Result<PathBuf> {
    if !path.exists() {
        bail!("sidecar file {} does not exist", path.display());
    }
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(env::current_dir()
            .context("failed to inspect the current directory")?
            .join(path))
    }
}

#[cfg(test)]
mod tests {
    use lifecore::NodeBodyExt;

    use super::*;

    fn test_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "lifecore-cli-state-test-{}-{}-{name}.json",
            process::id(),
            TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn state_round_trip_is_pretty_and_has_required_keys() {
        let path = test_path("round-trip");
        let state = StateFile::empty();
        state.create(&path).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("\n  \"lifecore\""));
        assert!(text.contains("\n  \"cli\""));
        let loaded = StateFile::load(&path).unwrap();
        assert_eq!(loaded.cli, state.cli);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn create_refuses_to_replace_existing_state() {
        let path = test_path("create-only");
        StateFile::empty().create(&path).unwrap();
        let before = fs::read(&path).unwrap();
        assert!(StateFile::empty().create(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn load_rejects_persisted_mock_suffix() {
        let path = test_path("mock");
        let mut state = StateFile::empty();
        state
            .lifecore
            .add_mock("Thing", NodeBodyExt::default(), None)
            .unwrap();
        fs::write(&path, serde_json::to_vec_pretty(&state).unwrap()).unwrap();

        assert!(StateFile::load(&path).is_err());
        fs::remove_file(path).unwrap();
    }
}
