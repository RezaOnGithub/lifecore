use crate::{AppError, proposal::FormatVersion};
use lifecore::{LifeCoreList, analysis::presentation::Dismissals};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions, TryLockError},
    io::{self, Write},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

pub const STATE_PATH_VARIABLE: &str = "LIFECORE_STATE_FILE_PATH";
pub const DEFAULT_STATE_FILE: &str = "LifeCoreState.json";
const BUSY_RETRY: Duration = Duration::from_secs(1);
const BUSY_POLL: Duration = Duration::from_millis(20);

//A The selected snapshot path and its stable sibling lock file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatePath {
    path: PathBuf,
}

impl StatePath {
    pub fn resolve() -> Result<Self, AppError> {
        let cwd = std::env::current_dir().map_err(|error| AppError::WorkingDirectory {
            message: error.to_string(),
        })?;
        Self::resolve_from(std::env::var_os(STATE_PATH_VARIABLE), &cwd)
    }

    //A A set variable always wins, even when empty or unusable: selection never
    //A falls back to the default or searches parent directories.
    pub fn resolve_from(variable: Option<OsString>, cwd: &Path) -> Result<Self, AppError> {
        let selected = match variable {
        Some(value) if value.is_empty() => return Err(AppError::EmptyStatePath),
        Some(value) => PathBuf::from(value),
        None => PathBuf::from(DEFAULT_STATE_FILE),
        };
        Ok(Self::new(cwd.join(selected)))
    }

    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn lock_path(&self) -> PathBuf {
        let mut lock = OsString::from(self.path.as_os_str());
        lock.push(".lock");
        PathBuf::from(lock)
    }

    fn directory(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new("."))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub format_version: FormatVersion<1>,
    pub graph_revision: Uuid,
    pub graph: LifeCoreList,
    pub dismissals: Dismissals,
}

impl Snapshot {
    pub fn new(graph: LifeCoreList) -> Self {
        Self {
            format_version: FormatVersion,
            graph_revision: Uuid::new_v4(),
            graph,
            dismissals: Dismissals::default(),
        }
    }
}

//A Reads open the path afresh and parse one complete file; atomic replacement
//A means they see either the old or the new snapshot. No lock is needed.
pub fn read(state: &StatePath) -> Result<Snapshot, AppError> {
    let text = fs::read_to_string(state.path()).map_err(|error| match error.kind() {
    io::ErrorKind::NotFound => AppError::DatabaseMissing {
        path: state.path().into(),
    },
    _ => AppError::DatabaseUnreadable {
        path: state.path().into(),
        message: error.to_string(),
    },
    })?;
    serde_json::from_str(&text).map_err(|error| AppError::DatabaseUnreadable {
        path: state.path().into(),
        message: error.to_string(),
    })
}

pub fn require_existing(state: &StatePath) -> Result<(), AppError> {
    match fs::metadata(state.path()) {
    Ok(_) => Ok(()),
    Err(error) if error.kind() == io::ErrorKind::NotFound => Err(AppError::DatabaseMissing {
        path: state.path().into(),
    }),
    Err(error) => Err(AppError::DatabaseUnreadable {
        path: state.path().into(),
        message: error.to_string(),
    }),
    }
}

//A Holding this value is writer exclusion. Closing the file releases the lock;
//A the lock file itself is never removed or replaced.
#[derive(Debug)]
pub struct WriterLock {
    _file: File,
}

pub fn lock(state: &StatePath) -> Result<WriterLock, AppError> {
    let path = state.lock_path();
    let failed = |error: io::Error| AppError::Lock {
        path: path.clone(),
        message: error.to_string(),
    };
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(failed)?;
    let deadline = Instant::now() + BUSY_RETRY;
    loop {
        match file.try_lock() {
        Ok(()) => return Ok(WriterLock { _file: file }),
        Err(TryLockError::WouldBlock) if Instant::now() < deadline => thread::sleep(BUSY_POLL),
        Err(TryLockError::WouldBlock) => return Err(AppError::Busy { lock: path }),
        Err(TryLockError::Error(error)) => return Err(failed(error)),
        }
    }
}

fn encode(snapshot: &Snapshot) -> Result<Vec<u8>, AppError> {
    let mut bytes =
        serde_json::to_vec_pretty(snapshot).map_err(|error| AppError::PublicationFailed {
            message: error.to_string(),
        })?;
    bytes.push(b'\n');
    Ok(bytes)
}

//A Writes and synchronizes a complete temporary file beside the target, so the
//A later rename or link never exposes a partial snapshot.
fn stage(state: &StatePath, bytes: &[u8]) -> Result<PathBuf, AppError> {
    let name = state
        .path()
        .file_name()
        .ok_or_else(|| AppError::PublicationFailed {
            message: format!("{} does not name a file", state.path().display()),
        })?;
    let mut temporary = OsString::from(".");
    temporary.push(name);
    temporary.push(format!(".{}.tmp", Uuid::new_v4()));
    let temporary = state.directory().join(temporary);
    let written = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .and_then(|mut file| {
            file.write_all(bytes)?;
            file.sync_all()
        });
    if let Err(error) = written {
        //A Best effort: a leftover temporary file does not affect the snapshot.
        fs::remove_file(&temporary).ok();
        return Err(AppError::PublicationFailed {
            message: error.to_string(),
        });
    }
    Ok(temporary)
}

fn sync_directory(state: &StatePath) -> Result<(), AppError> {
    File::open(state.directory())
        .and_then(|directory| directory.sync_all())
        .map_err(|error| AppError::PublicationUncertain {
            message: error.to_string(),
        })
}

//A Atomically replaces the published snapshot. Failures before the rename leave
//A the previous snapshot intact; a failure afterwards is reported as uncertain.
pub fn publish(state: &StatePath, snapshot: &Snapshot, _lock: &WriterLock) -> Result<(), AppError> {
    let temporary = stage(state, &encode(snapshot)?)?;
    if let Err(error) = fs::rename(&temporary, state.path()) {
        fs::remove_file(&temporary).ok();
        return Err(AppError::PublicationFailed {
            message: error.to_string(),
        });
    }
    sync_directory(state)
}

//A Initialization never replaces an existing path: a hard link fails if the
//A target exists, where a rename would silently clobber it.
pub fn publish_new(
    state: &StatePath,
    snapshot: &Snapshot,
    _lock: &WriterLock,
) -> Result<(), AppError> {
    let temporary = stage(state, &encode(snapshot)?)?;
    let linked = fs::hard_link(&temporary, state.path());
    fs::remove_file(&temporary).ok();
    match linked {
    Ok(()) => sync_directory(state),
    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Err(AppError::DatabaseExists {
        path: state.path().into(),
    }),
    Err(error) => Err(AppError::PublicationFailed {
        message: error.to_string(),
    }),
    }
}
