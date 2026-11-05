use crate::AppError;
use lifecore::{LifeCoreList, analysis::presentation::Dismissals};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions, Permissions, TryLockError},
    io::{self, Write},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;

pub const STATE_PATH_VARIABLE: &str = "LIFECORE_STATE_FILE_PATH";
pub const DEFAULT_STATE_FILE: &str = "LifeCoreState.json";
// A check under the default suite takes about two seconds at 4,000 entries
// and grows roughly quadratically (PERFORMANCE §1), so a waiting writer must
// outlast a competing apply rather than a brief write.
pub const DEFAULT_BUSY_RETRY: Duration = Duration::from_secs(10);
const BUSY_POLL: Duration = Duration::from_millis(20);

// The selected snapshot path and its stable sibling lock file.
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

    // A set variable always wins, even when empty or unusable: selection never
    // falls back to the default or searches parent directories.
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

    // A bare file name has an empty parent, which names no directory to open
    // or synchronize; it lives in the current directory.
    pub(crate) fn directory(&self) -> &Path {
        match self.path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub graph_revision: Uuid,
    pub graph: LifeCoreList,
    pub dismissals: Dismissals,
}

impl Snapshot {
    pub fn new(graph: LifeCoreList) -> Self {
        Self {
            graph_revision: Uuid::new_v4(),
            graph,
            dismissals: Dismissals::default(),
        }
    }
}

// Reads open the path afresh and parse one complete file; atomic replacement
// means they see either the old or the new snapshot. No lock is needed.
pub(crate) fn read(state: &StatePath) -> Result<Snapshot, AppError> {
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

pub(crate) fn require_existing(state: &StatePath) -> Result<(), AppError> {
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

// Initialization preconditions checked before the lock file is created, so a
// refused `init` leaves nothing behind. The no-clobber link in `publish_new`
// still decides a race with a concurrent `init`.
pub(crate) fn require_initializable(state: &StatePath) -> Result<(), AppError> {
    if fs::symlink_metadata(state.path()).is_ok() {
        return Err(AppError::DatabaseExists {
            path: state.path().into(),
        });
    }
    match fs::metadata(state.directory()) {
    Ok(metadata) if !metadata.is_dir() => Err(AppError::DirectoryMissing {
        path: state.directory().into(),
    }),
    Err(error) if error.kind() == io::ErrorKind::NotFound => Err(AppError::DirectoryMissing {
        path: state.directory().into(),
    }),
    _ => Ok(()),
    }
}

// Holding this value is writer exclusion over one state path. Closing the
// file releases the lock; the lock file itself is never removed or replaced.
#[derive(Debug)]
pub(crate) struct WriterLock {
    _file: File,
    state: StatePath,
}

// A snapshot read while holding writer exclusion, together with that lock.
// Only this can replace a published snapshot, so a mutation cannot store
// changes to a snapshot read before the lock was taken, or publish to a
// different path than it locked and read.
#[derive(Debug)]
pub(crate) struct LockedSnapshot {
    lock: WriterLock,
    snapshot: Snapshot,
}

impl WriterLock {
    pub(crate) fn read(self) -> Result<LockedSnapshot, AppError> {
        let snapshot = read(&self.state)?;
        Ok(LockedSnapshot {
            lock: self,
            snapshot,
        })
    }

    // Initialization never replaces an existing path: a hard link fails if the
    // target exists, where a rename would silently clobber it. This requires a
    // filesystem with hard links.
    pub(crate) fn publish_new(
        &self,
        snapshot: &Snapshot,
        sync: SyncDirectory,
    ) -> Result<Publication, AppError> {
        let state = &self.state;
        let temporary = stage(state, &encode(snapshot)?, None)?;
        let linked = fs::hard_link(&temporary, state.path());
        fs::remove_file(&temporary).ok();
        match linked {
        Ok(()) => Ok(synchronize(state, sync)),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            Err(AppError::DatabaseExists {
                path: state.path().into(),
            })
        }
        Err(error) => Err(AppError::PublicationFailed {
            message: error.to_string(),
        }),
        }
    }
}

impl LockedSnapshot {
    pub(crate) fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }
    pub(crate) fn snapshot_mut(&mut self) -> &mut Snapshot {
        &mut self.snapshot
    }

    // Atomically replaces the published snapshot, keeping its file mode.
    // Failures before the rename leave the previous snapshot intact; a failure
    // afterwards is an uncertain publication, not a rollback.
    pub(crate) fn publish(&self, sync: SyncDirectory) -> Result<Publication, AppError> {
        let state = &self.lock.state;
        let permissions = fs::metadata(state.path())
            .ok()
            .filter(|metadata| metadata.is_file())
            .map(|metadata| metadata.permissions());
        let temporary = stage(state, &encode(&self.snapshot)?, permissions)?;
        if let Err(error) = fs::rename(&temporary, state.path()) {
            fs::remove_file(&temporary).ok();
            return Err(AppError::PublicationFailed {
                message: error.to_string(),
            });
        }
        Ok(synchronize(state, sync))
    }
}

pub(crate) fn lock(state: &StatePath, retry: Duration) -> Result<WriterLock, AppError> {
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
    let deadline = Instant::now() + retry;
    loop {
        match file.try_lock() {
        Ok(()) => {
            return Ok(WriterLock {
                _file: file,
                state: state.clone(),
            });
        }
        Err(TryLockError::WouldBlock) if Instant::now() < deadline => thread::sleep(BUSY_POLL),
        Err(TryLockError::WouldBlock) => return Err(AppError::Busy { lock: path }),
        Err(TryLockError::Error(error)) => return Err(failed(error)),
        }
    }
}

// Synchronizes the containing directory after replacement. The service holds
// it as a function so tests can inject a failure after the rename.
pub(crate) type SyncDirectory = fn(&Path) -> io::Result<()>;

pub(crate) fn sync_directory(directory: &Path) -> io::Result<()> {
    File::open(directory)?.sync_all()
}

// Whether a publication that replaced the snapshot was also synchronized. An
// uncertain one may or may not persist, but the new state is already visible.
#[must_use]
pub(crate) enum Publication {
    Complete,
    Uncertain(String),
}

fn encode(snapshot: &Snapshot) -> Result<Vec<u8>, AppError> {
    let mut bytes =
        serde_json::to_vec_pretty(snapshot).map_err(|error| AppError::PublicationFailed {
            message: error.to_string(),
        })?;
    bytes.push(b'\n');
    Ok(bytes)
}

// Writes and synchronizes a complete temporary file beside the target, so the
// later rename or link never exposes a partial snapshot. Permissions are set
// before any content is written, so a private snapshot never has a readable
// temporary copy.
fn stage(
    state: &StatePath,
    bytes: &[u8],
    permissions: Option<Permissions>,
) -> Result<PathBuf, AppError> {
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
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| AppError::PublicationFailed {
            message: error.to_string(),
        })?;
    let written = permissions
        .map_or(Ok(()), |permissions| file.set_permissions(permissions))
        .and_then(|()| file.write_all(bytes))
        .and_then(|()| file.sync_all());
    if let Err(error) = written {
        // Best effort: a leftover temporary file does not affect the snapshot.
        fs::remove_file(&temporary).ok();
        return Err(AppError::PublicationFailed {
            message: error.to_string(),
        });
    }
    Ok(temporary)
}

fn synchronize(state: &StatePath, sync: SyncDirectory) -> Publication {
    match sync(state.directory()) {
    Ok(()) => Publication::Complete,
    Err(error) => Publication::Uncertain(error.to_string()),
    }
}
