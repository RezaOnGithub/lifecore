use serde::{Deserialize, Serialize};

use crate::{
    Body, Edge, Entry, EntryId, LifeCoreError, LifeCoreList, Node, View,
    analysis::{AnalysisOutput, AnalysisRunner, SuiteConfig},
    entries::present,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reference {
    Existing(EntryId),
    // Zero-based operation index within this plan, resolved only after that
    // operation succeeds. It is bounded by the size of one plan, so it stays a
    // JSON number, unlike entry positions.
    Added(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevisionIntent {
    Revise,
    Branch,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewNode {
    #[serde(rename = "type")]
    pub ty_name: String,
    #[serde(
        rename = "predecessor",
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub parent: Option<Reference>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub intent: Option<RevisionIntent>,
    #[serde(rename = "payload")]
    pub body: Body,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewEdge {
    #[serde(rename = "type")]
    pub ty_name: String,
    pub from: Reference,
    pub to: Reference,
}

// A node or edge operation, and never a suite change. A lowered proposal and
// a suggestion's actions are both `Vec<EntryChange>`, so comparing them is
// plain equality. It serializes like a proposal entry without its name.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum EntryChange {
    #[serde(rename = "node")]
    AddNode(NewNode),
    #[serde(rename = "edge")]
    AddEdge(NewEdge),
}

// The core staging input: no names or encodings, earlier operations by index,
// and no identity until `prepare` assigns one. Only `init` and `add-analyzer`
// build `AddAnalyzer`. No file or payload carries one, so it has no Serde form.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Change {
    Entry(EntryChange),
    AddAnalyzer { name: String, config: String },
}

impl From<EntryChange> for Change {
    fn from(change: EntryChange) -> Self {
        Self::Entry(change)
    }
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
}

// The outcome of a full check. Only an accepted plan owns a candidate graph
// and can commit it. A rejection keeps the complete analysis and the
// provisional operation mapping but no graph, so a state that failed its
// checks cannot be committed, published, or inspected as history.
#[derive(Debug)]
pub enum Checked {
    Accepted(PreparedTransaction),
    Rejected(Rejection),
}

impl Checked {
    pub fn analysis(&self) -> &AnalysisOutput {
        match self {
        Self::Accepted(prepared) => &prepared.analysis,
        Self::Rejected(rejection) => &rejection.analysis,
        }
    }
    // One slot per change, in plan order; configuration changes append no entry.
    pub fn operations(&self) -> &[Option<EntryId>] {
        match self {
        Self::Accepted(prepared) => &prepared.operations,
        Self::Rejected(rejection) => &rejection.operations,
        }
    }
    // For a caller that reports the analysis without committing.
    pub fn into_analysis(self) -> AnalysisOutput {
        match self {
        Self::Accepted(prepared) => prepared.analysis,
        Self::Rejected(rejection) => rejection.analysis,
        }
    }
    pub fn accepted(self) -> Result<PreparedTransaction, Box<Rejection>> {
        match self {
        Self::Accepted(prepared) => Ok(prepared),
        Self::Rejected(rejection) => Err(Box::new(rejection)),
        }
    }
}

#[derive(Debug)]
pub struct Rejection {
    analysis: AnalysisOutput,
    operations: Vec<Option<EntryId>>,
}

impl Rejection {
    pub fn analysis(&self) -> &AnalysisOutput {
        &self.analysis
    }
    pub fn operations(&self) -> &[Option<EntryId>] {
        &self.operations
    }
}

#[derive(Debug)]
pub struct PreparedTransaction {
    // Commit requires the receiver to match the original state, preventing a
    // concurrently prepared candidate from overwriting intervening changes.
    base_end_exclusive: usize,
    original_suite: SuiteConfig,
    candidate: LifeCoreList,
    analysis: AnalysisOutput,
    operations: Vec<Option<EntryId>>,
}

impl PreparedTransaction {
    pub fn view(&self) -> View<'_> {
        self.candidate.view()
    }
    pub fn analysis(&self) -> &AnalysisOutput {
        &self.analysis
    }
    pub fn operations(&self) -> &[Option<EntryId>] {
        &self.operations
    }
    pub fn added(&self) -> Vec<EntryId> {
        self.operations.iter().flatten().copied().collect()
    }

    // Returns the analysis that certified the committed graph, so a caller can
    // report it without keeping a copy.
    pub fn commit(self, receiver: &mut LifeCoreList) -> Result<AnalysisOutput, Error> {
        // The candidate retains the exact original prefix, so only the original
        // boundary and ordered suite need a separate snapshot. Compare contents,
        // not serialization or hashes, including changes that append no entries.
        if receiver.view().len() != self.base_end_exclusive
            || self.candidate.view().extends(receiver.view()).is_err()
            || receiver.analysis_suite() != &self.original_suite
        {
            return Err(Error::StaleBase);
        }
        *receiver = self.candidate;
        Ok(self.analysis)
    }
}

pub fn prepare(
    graph: &LifeCoreList,
    changes: &[Change],
    runner: &mut AnalysisRunner,
) -> Result<Checked, Error> {
    graph
        .basic_validation()
        .map_err(LifeCoreError::BasicValidationFailure)?;
    let mut candidate = graph.clone();
    let origin = uuid::Uuid::new_v4();
    let mut operations = Vec::<Option<EntryId>>::new();
    // One isolated clone owns the entire suffix. No public unchecked mutation or
    // rollback journal.
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
            position: candidate.view().len(),
        };
        let entry = match change {
        Change::Entry(EntryChange::AddNode(node)) => Some(Entry::Node(Node {
            id,
            ty_name: node.ty_name.clone(),
            body: node.body.clone(),
            parent: node.parent.map(resolve).transpose()?,
        })),
        Change::Entry(EntryChange::AddEdge(edge)) => Some(Entry::Edge(Edge {
            id,
            ty_name: edge.ty_name.clone(),
            from: resolve(edge.from)?,
            to: resolve(edge.to)?,
        })),
        Change::AddAnalyzer { name, config } => {
            candidate.stage_analyzer(crate::analysis::Config {
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
        candidate
            .stage(entry)
            .map_err(LifeCoreError::BasicValidationFailure)?;
        // Intent is judged in the view immediately before this append, which
        // includes earlier operations of the same plan.
        if let Change::Entry(EntryChange::AddNode(node)) = change {
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
        operations.push(Some(id));
    }
    // Full replay certifies old history and every candidate prefix under the
    // resulting suite. Any inconsistency rejects the plan and drops the
    // candidate; its analysis stays complete for reporting.
    let analysis = runner.run(&candidate)?;
    if analysis.has_error() {
        return Ok(Checked::Rejected(Rejection {
            analysis,
            operations,
        }));
    }
    Ok(Checked::Accepted(PreparedTransaction {
        base_end_exclusive: graph.view().len(),
        original_suite: graph.analysis_suite().clone(),
        candidate,
        analysis,
        operations,
    }))
}

#[cfg(test)]
mod test;
