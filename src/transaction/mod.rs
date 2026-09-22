use serde::{Deserialize, Serialize};

use crate::{
    Body, Edge, Entry, EntryId, LifeCoreError, LifeCoreList, Node, View,
    analysis::{AnalysisOutput, AnalysisRunner, SuiteConfig},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    #[error("the database changed since this transaction was prepared")]
    StaleBase,
    #[error("plan reference {operation} does not name an earlier entry operation")]
    InvalidPlanReference { operation: usize },
}

pub struct PreparedTransaction {
    //A Commit requires the receiver to match the original state, preventing a
    //A concurrently prepared candidate from overwriting intervening changes.
    base_end_exclusive: usize,
    original_suite: SuiteConfig,
    candidate: LifeCoreList,
    pub diagnostics: AnalysisOutput,
    added: Vec<EntryId>,
}

impl PreparedTransaction {
    pub fn view(&self) -> View<'_> {
        self.candidate.view()
    }
    pub fn added(&self) -> &[EntryId] {
        &self.added
    }

    pub fn commit(self, receiver: &mut LifeCoreList) -> Result<(), Error> {
        //A The candidate retains the exact original prefix, so only the original
        //A boundary and ordered suite need a separate snapshot. Compare contents,
        //A not serialization or hashes, including changes that append no entries.
        if receiver.view().len() != self.base_end_exclusive
            || self.candidate.view().extends(receiver.view()).is_err()
            || receiver.analysis_suite() != &self.original_suite
        {
            return Err(Error::StaleBase);
        }
        *receiver = self.candidate;
        Ok(())
    }
}

pub fn prepare(
    graph: &LifeCoreList,
    changes: &[Change],
    runner: &mut AnalysisRunner,
) -> Result<PreparedTransaction, Error> {
    graph
        .basic_validation()
        .map_err(LifeCoreError::BasicValidationFailure)?;
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
    Ok(PreparedTransaction {
        base_end_exclusive: graph.view().len(),
        original_suite: graph.analysis_suite().clone(),
        candidate,
        diagnostics,
        added,
    })
}

#[cfg(test)]
mod test;
