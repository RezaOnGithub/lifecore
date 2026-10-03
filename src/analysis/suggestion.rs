use super::{AnalysisRunner, EvaluatedView, MessageKind, Observation};
use crate::{
    LifeCoreList, ViewError,
    transaction::{self, Change, Checked},
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the selected observation has no suggested actions")]
    NotSuggestion,
    #[error(transparent)]
    View(#[from] ViewError),
    #[error(transparent)]
    Check(#[from] transaction::Error),
}

//A Selecting an observation is explicit. Its ordered actions are checked anew
//A against the supplied base and runner inputs; checking never commits them.
//A Revision intent is validated by `transaction::prepare` like any other plan.
pub fn prepare_suggestion(
    graph: &LifeCoreList,
    observation: &Observation,
    runner: &mut AnalysisRunner,
) -> Result<Checked, Error> {
    if graph.view().id().database != observation.key.database {
        return Err(ViewError::DifferentHistory.into());
    }
    if let EvaluatedView::Prefix(at) = observation.evaluated_view
        && graph.view().prefix(at.end_exclusive)?.id() != at
    {
        return Err(ViewError::DifferentHistory.into());
    }
    let MessageKind::Suggestion(actions) = &observation.kind else {
        return Err(Error::NotSuggestion);
    };
    let changes: Vec<Change> = actions.iter().cloned().map(Change::from).collect();
    Ok(transaction::prepare(graph, &changes, runner)?)
}
