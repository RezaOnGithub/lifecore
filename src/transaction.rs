use std::{
    fs::{File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::{
    Body, EntryIndex, LifeCore, LifeCoreError, LifeCoreList,
    analysis::{AnalysisRunner, Output},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Change {
    AddNode {
        ty_name: String,
        body: Body,
        parent: Option<EntryIndex>,
    },
    AddEdge {
        from: EntryIndex,
        ty_name: String,
        to: EntryIndex,
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
    #[error("persisted history contains provisional entries")]
    ProvisionalHistory,
}

pub struct CheckedPlan {
    base: Vec<u8>,
    candidate: LifeCoreList,
    pub diagnostics: Output,
}

impl CheckedPlan {
    pub fn graph(&self) -> &LifeCoreList {
        &self.candidate
    }

    pub fn commit(mut self, receiver: &mut LifeCoreList) -> Result<(), Error> {
        if serde_json::to_vec(receiver)? != self.base {
            return Err(Error::StaleBase);
        }
        self.candidate.apply()?;
        *receiver = self.candidate;
        Ok(())
    }
}

pub fn check(
    graph: &LifeCoreList,
    changes: &[Change],
    runner: &mut AnalysisRunner<LifeCoreList>,
) -> Result<CheckedPlan, Error> {
    graph
        .basic_validation()
        .map_err(LifeCoreError::BasicValidationFailure)?;
    let base = serde_json::to_vec(graph)?;
    let mut candidate = graph.clone();
    //A The candidate owns every staged change. A later failure discards the entire
    // plan.
    for change in changes {
        match change {
        Change::AddNode {
            ty_name,
            body,
            parent,
        } => {
            candidate.add_mock(
                ty_name,
                body.clone(),
                parent.map(|id| id.before_and_including()),
            )?;
        }
        Change::AddEdge { from, ty_name, to } => {
            candidate.connect_mock(
                from.before_and_including(),
                ty_name,
                to.before_and_including(),
            )?;
        }
        Change::AddAnalyzer { name, config } => candidate.add_analyzer(name, config),
        }
    }
    //A Replay includes the existing history and every proposed entry boundary.
    let diagnostics = runner.run(&candidate)?;
    if diagnostics.has_error() {
        return Err(Error::Core(LifeCoreError::SystemsError(
            diagnostics.errors(),
        )));
    }
    Ok(CheckedPlan {
        base,
        candidate,
        diagnostics,
    })
}

pub struct Store {
    path: PathBuf,
    #[cfg(test)]
    fail_replace: bool,
}

impl Store {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            #[cfg(test)]
            fail_replace: false,
        }
    }

    pub fn load(&self) -> Result<LifeCoreList, Error> {
        let graph: LifeCoreList = serde_json::from_reader(File::open(&self.path)?)?;
        Self::validate(&graph)?;
        Ok(graph)
    }

    pub fn initialize(&self, graph: &LifeCoreList) -> Result<(), Error> {
        Self::validate(graph)?;
        let _lock = self.lock()?;
        self.write_temporary(graph)?
            .persist_noclobber(&self.path)
            .map_err(|e| e.error)?;
        Ok(())
    }

    pub fn commit(&self, mut plan: CheckedPlan) -> Result<LifeCoreList, Error> {
        //A All cooperating writers hold this lock from base verification through
        // rename.
        let _lock = self.lock()?;
        if serde_json::to_vec(&self.load()?)? != plan.base {
            return Err(Error::StaleBase);
        }
        plan.candidate.apply()?;
        Self::validate(&plan.candidate)?;
        let temporary = self.write_temporary(&plan.candidate)?;
        #[cfg(test)]
        if self.fail_replace {
            return Err(std::io::Error::other("injected replacement failure").into());
        }
        temporary.persist(&self.path).map_err(|e| e.error)?;
        Ok(plan.candidate)
    }

    fn validate(graph: &LifeCoreList) -> Result<(), Error> {
        graph
            .basic_validation()
            .map_err(LifeCoreError::BasicValidationFailure)?;
        if graph.is_mock() {
            return Err(Error::ProvisionalHistory);
        }
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

    fn write_temporary(&self, graph: &LifeCoreList) -> Result<NamedTempFile, Error> {
        let parent = self
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut temporary = NamedTempFile::new_in(parent)?;
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
