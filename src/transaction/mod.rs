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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevisionIntent {
    Revise,
    Branch,
}

//A Node and edge operations are shared with suggested actions, so comparing a
//A lowered proposal with a suggestion is plain equality.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewNode {
    pub ty_name: String,
    #[serde(deserialize_with = "crate::deserialize_body")]
    pub body: Body,
    pub parent: Option<Reference>,
    pub intent: Option<RevisionIntent>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewEdge {
    pub from: Reference,
    pub ty_name: String,
    pub to: Reference,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Change {
    AddNode(NewNode),
    AddEdge(NewEdge),
    AddAnalyzer { name: String, config: String },
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Core(#[from] LifeCoreError),
    #[error("the database changed since this transaction was prepared")]
    StaleBase,
    #[error("plan reference {operation} does not name an earlier entry operation")]
    InvalidPlanReference { operation: usize },
    #[error("operation {operation} names a predecessor without a revision intent")]
    MissingIntent { operation: usize },
    #[error("operation {operation} creates a root but declares a revision intent")]
    UnexpectedIntent { operation: usize },
    #[error("operation {operation} revises a predecessor that is not current")]
    RevisesSuperseded { operation: usize },
    #[error("analysis emitted blocking inconsistencies")]
    Inconsistent(Box<CheckedCandidate>),
}

//A A fully staged and replayed candidate. It stays inspectable even when its
//A analysis contains inconsistencies, but only `into_prepared` can make it
//A committable, and only when there are none.
#[derive(Debug)]
pub struct CheckedCandidate {
    //A Commit requires the receiver to match the original state, preventing a
    //A concurrently prepared candidate from overwriting intervening changes.
    base_end_exclusive: usize,
    original_suite: SuiteConfig,
    candidate: LifeCoreList,
    analysis: AnalysisOutput,
    operations: Vec<Option<EntryId>>,
    added: Vec<EntryId>,
}

impl CheckedCandidate {
    pub fn view(&self) -> View<'_> {
        self.candidate.view()
    }
    pub fn analysis_suite(&self) -> &SuiteConfig {
        self.candidate.analysis_suite()
    }
    pub fn analysis(&self) -> &AnalysisOutput {
        &self.analysis
    }
    pub fn base_end_exclusive(&self) -> usize {
        self.base_end_exclusive
    }
    //A One slot per change, in plan order; configuration changes append no entry.
    pub fn operations(&self) -> &[Option<EntryId>] {
        &self.operations
    }
    pub fn added(&self) -> &[EntryId] {
        &self.added
    }
    pub fn into_prepared(self) -> Result<PreparedTransaction, Error> {
        if self.analysis.has_error() {
            return Err(Error::Inconsistent(Box::new(self)));
        }
        Ok(PreparedTransaction(self))
    }
}

pub struct PreparedTransaction(CheckedCandidate);

impl PreparedTransaction {
    pub fn view(&self) -> View<'_> {
        self.0.view()
    }
    pub fn analysis(&self) -> &AnalysisOutput {
        self.0.analysis()
    }
    pub fn operations(&self) -> &[Option<EntryId>] {
        self.0.operations()
    }
    pub fn added(&self) -> &[EntryId] {
        self.0.added()
    }

    pub fn commit(self, receiver: &mut LifeCoreList) -> Result<(), Error> {
        let checked = self.0;
        //A The candidate retains the exact original prefix, so only the original
        //A boundary and ordered suite need a separate snapshot. Compare contents,
        //A not serialization or hashes, including changes that append no entries.
        if receiver.view().len() != checked.base_end_exclusive
            || checked.candidate.view().extends(receiver.view()).is_err()
            || receiver.analysis_suite() != &checked.original_suite
        {
            return Err(Error::StaleBase);
        }
        *receiver = checked.candidate;
        Ok(())
    }
}

pub fn prepare(
    graph: &LifeCoreList,
    changes: &[Change],
    runner: &mut AnalysisRunner,
) -> Result<CheckedCandidate, Error> {
    graph
        .basic_validation()
        .map_err(LifeCoreError::BasicValidationFailure)?;
    let mut candidate = graph.clone();
    let origin = uuid::Uuid::new_v4();
    let mut operations = Vec::<Option<EntryId>>::new();
    let mut added = vec![];
    //A One isolated clone owns the entire suffix. No public unchecked mutation or
    //A rollback journal.
    for (operation, change) in changes.iter().enumerate() {
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
        Change::AddNode(node) => Some(Entry::Node(Node {
            id,
            ty_name: node.ty_name.clone(),
            body: node.body.clone(),
            parent: node.parent.map(resolve).transpose()?,
        })),
        Change::AddEdge(edge) => Some(Entry::Edge(Edge {
            id,
            ty_name: edge.ty_name.clone(),
            from: resolve(edge.from)?,
            to: resolve(edge.to)?,
        })),
        Change::AddAnalyzer { name, config } => {
            candidate.suite.0.push(crate::analysis::Config {
                name: name.clone(),
                config: config.clone(),
            });
            None
        }
        };
        let Some(entry) = entry else {
            operations.push(None);
            continue;
        };
        let parent = entry.as_node().and_then(|node| node.parent);
        candidate.entries.push(entry);
        candidate
            .basic_validation()
            .map_err(LifeCoreError::BasicValidationFailure)?;
        //A Intent is judged in the view immediately before this append, which
        //A includes earlier operations of the same plan.
        if let Change::AddNode(node) = change {
            let before = candidate.view().prefix(id.position).expect("staged entry");
            match (parent, node.intent) {
            (Some(_), None) => return Err(Error::MissingIntent { operation }),
            (None, Some(_)) => return Err(Error::UnexpectedIntent { operation }),
            (Some(parent), Some(RevisionIntent::Revise))
                if before.is_stale(parent) != Some(false) =>
            {
                return Err(Error::RevisesSuperseded { operation });
            }
            _ => {}
            }
        }
        added.push(id);
        operations.push(Some(id));
    }
    //A Full replay certifies old history and every candidate prefix under the
    //A resulting suite. Inconsistencies stay in the output for inspection.
    let analysis = runner.run(&candidate)?;
    Ok(CheckedCandidate {
        base_end_exclusive: graph.view().len(),
        original_suite: graph.analysis_suite().clone(),
        candidate,
        analysis,
        operations,
        added,
    })
}

#[cfg(test)]
mod test;
