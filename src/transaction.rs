use std::{
    fs::{File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::{
    Body, Edge, Entry, EntryId, LifeCoreError, LifeCoreList, Node, View,
    analysis::{AnalysisRunner, Output},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reference {
    Existing(EntryId),
    //A Zero-based operation index within this plan, resolved only after that operation succeeds.
    Added(usize),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Change {
    AddNode {
        ty_name: String,
        body: Body,
        parent: Option<Reference>,
    },
    AddEdge {
        from: Reference,
        ty_name: String,
        to: Reference,
    },
    AddAnalyzer {
        name: String,
        config: String,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Core(#[from] LifeCoreError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Encoding(#[from] serde_json::Error),
    #[error("the database changed since this plan was checked")]
    StaleBase,
    #[error("state was published but directory synchronization failed: {0}")]
    Published(#[source] std::io::Error),
    #[error("plan reference {operation} does not name an earlier entry operation")]
    InvalidPlanReference { operation: usize },
}

pub struct CheckedPlan {
    base: Vec<u8>,
    candidate: LifeCoreList,
    pub diagnostics: Output,
    added: Vec<EntryId>,
}

impl CheckedPlan {
    pub fn view(&self) -> View<'_> {
        self.candidate.view()
    }
    pub fn added(&self) -> &[EntryId] {
        &self.added
    }

    pub fn commit(self, receiver: &mut LifeCoreList) -> Result<(), Error> {
        if serde_json::to_vec(receiver)? != self.base {
            return Err(Error::StaleBase);
        }
        *receiver = self.candidate;
        Ok(())
    }
}

pub fn check(
    graph: &LifeCoreList,
    changes: &[Change],
    runner: &mut AnalysisRunner,
) -> Result<CheckedPlan, Error> {
    graph
        .basic_validation()
        .map_err(LifeCoreError::BasicValidationFailure)?;
    let base = serde_json::to_vec(graph)?;
    let mut candidate = graph.clone();
    let origin = uuid::Uuid::new_v4();
    let mut operations = Vec::<Option<EntryId>>::new();
    let mut added = vec![];
    //A One isolated clone owns the entire suffix. No public unchecked mutation or
    //A rollback journal.
    for change in changes {
        let resolve = |reference: Reference| match reference {
        Reference::Existing(id) => Ok(id),
        Reference::Added(operation) => operations
            .get(operation)
            .copied()
            .flatten()
            .ok_or(Error::InvalidPlanReference { operation }),
        };
        let id = EntryId {
            origin,
            position: candidate.entries.len(),
        };
        let entry = match change {
        Change::AddNode {
            ty_name,
            body,
            parent,
        } => Some(Entry::Node(Node {
            id,
            ty_name: ty_name.clone(),
            body: body.clone(),
            parent: parent.map(resolve).transpose()?,
        })),
        Change::AddEdge { from, ty_name, to } => Some(Entry::Edge(Edge {
            id,
            ty_name: ty_name.clone(),
            from: resolve(*from)?,
            to: resolve(*to)?,
        })),
        Change::AddAnalyzer { name, config } => {
            candidate.suite.0.push(crate::analysis::Config {
                name: name.clone(),
                config: config.clone(),
            });
            None
        }
        };
        if let Some(entry) = entry {
            candidate.entries.push(entry);
            candidate
                .basic_validation()
                .map_err(LifeCoreError::BasicValidationFailure)?;
            added.push(id);
            operations.push(Some(id));
        } else {
            operations.push(None);
        }
    }
    //A Full replay certifies old history and every candidate prefix under the
    //A resulting suite.
    let diagnostics = runner.run(&candidate)?.into_result()?;
    Ok(CheckedPlan {
        base,
        candidate,
        diagnostics,
        added,
    })
}

pub struct Store {
    path: PathBuf,
    #[cfg(test)]
    fail_replace: bool,
    #[cfg(test)]
    fail_directory_sync: bool,
}

impl Store {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            #[cfg(test)]
            fail_replace: false,
            #[cfg(test)]
            fail_directory_sync: false,
        }
    }

    pub fn load(&self) -> Result<LifeCoreList, Error> {
        let graph: LifeCoreList = serde_json::from_reader(File::open(&self.path)?)?;
        Self::validate(&graph)?;
        Ok(graph)
    }

    pub fn initialize(
        &self,
        graph: &LifeCoreList,
        runner: &mut AnalysisRunner,
    ) -> Result<(), Error> {
        Self::validate(graph)?;
        runner.run(graph)?.into_result()?;
        let _lock = self.lock()?;
        self.write_temporary(graph)?
            .persist_noclobber(&self.path)
            .map_err(|e| e.error)?;
        self.sync_directory()?;
        Ok(())
    }

    pub fn commit(&self, plan: CheckedPlan) -> Result<LifeCoreList, Error> {
        //A All cooperating writers hold this lock from base verification through
        //A rename.
        let _lock = self.lock()?;
        if serde_json::to_vec(&self.load()?)? != plan.base {
            return Err(Error::StaleBase);
        }
        Self::validate(&plan.candidate)?;
        let temporary = self.write_temporary(&plan.candidate)?;
        #[cfg(test)]
        if self.fail_replace {
            return Err(std::io::Error::other("injected replacement failure").into());
        }
        temporary.persist(&self.path).map_err(|e| e.error)?;
        self.sync_directory()?;
        Ok(plan.candidate)
    }

    fn validate(graph: &LifeCoreList) -> Result<(), Error> {
        graph
            .basic_validation()
            .map_err(LifeCoreError::BasicValidationFailure)?;
        Ok(())
    }

    fn lock(&self) -> Result<File, Error> {
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

    fn directory(&self) -> &Path {
        self.path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
    }

    fn sync_directory(&self) -> Result<(), Error> {
        #[cfg(test)]
        if self.fail_directory_sync {
            return Err(Error::Published(std::io::Error::other(
                "injected directory-sync failure",
            )));
        }
        //A A successful rename is already visible. Report uncertainty distinctly if
        //A durability fails.
        File::open(self.directory())
            .and_then(|directory| directory.sync_all())
            .map_err(Error::Published)
    }

    fn write_temporary(&self, graph: &LifeCoreList) -> Result<NamedTempFile, Error> {
        let mut temporary = NamedTempFile::new_in(self.directory())?;
        {
            let mut writer = BufWriter::new(temporary.as_file_mut());
            serde_json::to_writer_pretty(&mut writer, graph)?;
            writer.write_all(b"\n")?;
            writer.flush()?;
        }
        temporary.as_file().sync_all()?;
        Ok(temporary)
    }
}

#[cfg(test)]
mod test {
    mod test2;
}
