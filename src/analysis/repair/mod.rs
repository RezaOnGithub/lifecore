use super::{AnalysisRunner, MessageKind, Observation};
use crate::{
    Body, LifeCoreList, ViewError,
    transaction::{self, Change, PreparedTransaction, Reference},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevisionIntent {
    Revise,
    Branch,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SuggestedAction {
    AddNode {
        ty_name: String,
        payload: Body,
        parent: Option<Reference>,
        intent: Option<RevisionIntent>,
    },
    AddEdge {
        from: Reference,
        ty_name: String,
        to: Reference,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the selected observation has no suggested repair")]
    NotSuggestion,
    #[error(transparent)]
    View(#[from] ViewError),
    #[error(transparent)]
    Check(#[from] transaction::Error),
    #[error(
        "repair operation {operation} has inconsistent predecessor/intent or revises a stale predecessor"
    )]
    Intent { operation: usize },
}

//A Selecting an observation is explicit. Its ordered actions are checked anew
//A against the supplied base and runner inputs; checking never commits them.
pub fn prepare_suggestion(
    graph: &LifeCoreList,
    observation: &Observation,
    runner: &mut AnalysisRunner,
) -> Result<PreparedTransaction, Error> {
    if graph.view().id().database != observation.key.database {
        return Err(ViewError::DifferentHistory.into());
    }
    if let Some(at) = observation.evaluated_view
        && graph.view().prefix(at.end_exclusive)?.id() != at
    {
        return Err(ViewError::DifferentHistory.into());
    }
    let MessageKind::Suggestion(actions) = &observation.kind else {
        return Err(Error::NotSuggestion);
    };
    let changes: Vec<_> = actions
        .iter()
        .map(|action| match action {
        SuggestedAction::AddNode {
            ty_name,
            payload,
            parent,
            ..
        } => Change::AddNode {
            ty_name: ty_name.clone(),
            body: payload.clone(),
            parent: *parent,
        },
        SuggestedAction::AddEdge { from, ty_name, to } => Change::AddEdge {
            from: *from,
            ty_name: ty_name.clone(),
            to: *to,
        },
        })
        .collect();
    let transaction = transaction::prepare(graph, &changes, runner)?;
    for (operation, (action, id)) in actions.iter().zip(transaction.added()).enumerate() {
        if let SuggestedAction::AddNode { intent, .. } = action {
            let parent = transaction.view().node(*id).expect("prepared node").parent;
            let before = transaction.view().prefix(id.position())?;
            if parent.is_some() != intent.is_some()
                || (*intent == Some(RevisionIntent::Revise)
                    && parent.is_some_and(|id| before.is_stale(id) != Some(false)))
            {
                return Err(Error::Intent { operation });
            }
        }
    }
    Ok(transaction)
}
