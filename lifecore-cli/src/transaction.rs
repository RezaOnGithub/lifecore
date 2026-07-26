use std::str::FromStr;

use lifecore::{Body, EntryIndex, LifeCore};
use serde::Serialize;

use crate::{
    analysis::{AnalyzerRegistry, run_analysis},
    app::{AnalyzerRecord, EntryRecord, ViewSummary},
    entry::entry_record,
    error::AppError,
    locator::{EntryExpectation, EntryLocator, resolve_index},
    messages::{MessageRecord, MessageUniverse, Severity},
    state::{DismissalRecord, MESSAGE_ID_VERSION, PersistedState, StateStore},
    view::GraphView,
};

#[derive(Clone, Debug)]
pub enum PlannedChange {
    AddNode {
        type_name: String,
        body: Body,
        parent: Option<EntryIndex>,
    },
    AddEdge {
        from: EntryIndex,
        type_name: String,
        to: EntryIndex,
    },
    AddAnalyzer {
        name: String,
        config: String,
    },
    Composite(Vec<PlannedChange>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExpectedHead {
    Empty,
    Entry(EntryLocator),
}

impl FromStr for ExpectedHead {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
        "EMPTY" => Ok(Self::Empty),
        "HEAD" => Err(AppError::invalid_expected_head(
            "HEAD is dynamic; use EMPTY or a stable entry locator",
        )),
        value => EntryLocator::parse(value).map(Self::Entry),
        }
    }
}

#[derive(Clone, Debug)]
pub struct TransactionRequest {
    pub change: PlannedChange,
    pub commit: bool,
    pub expected_head: Option<ExpectedHead>,
    pub dismiss_after_commit: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionState {
    DryRun,
    Committed,
    Aborted,
}

impl TransactionState {
    pub fn label(self) -> &'static str {
        match self {
        Self::DryRun => "dry run",
        Self::Committed => "committed",
        Self::Aborted => "aborted",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TransactionResult {
    pub state: TransactionState,
    pub entries: Vec<EntryRecord>,
    pub analyzers: Vec<AnalyzerRecord>,
    pub messages: Vec<MessageRecord>,
    pub view: ViewSummary,
}

pub struct TransactionService<'a> {
    store: &'a StateStore,
    registry: &'a AnalyzerRegistry,
    #[cfg(test)]
    fail_apply: bool,
}

impl<'a> TransactionService<'a> {
    pub fn new(store: &'a StateStore, registry: &'a AnalyzerRegistry) -> Self {
        Self {
            store,
            registry,
            #[cfg(test)]
            fail_apply: false,
        }
    }

    pub fn execute(
        &self,
        loaded: &PersistedState,
        request: TransactionRequest,
    ) -> Result<TransactionResult, AppError> {
        if request.expected_head.is_some() && !request.commit {
            return Err(AppError::invalid_expected_head(
                "--expect-head requires --commit",
            ));
        }

        let original_end = graph_end(loaded);
        let original_analyzer_count = loaded.lifecore.current_analysis_suite().0.len();
        let mut candidate = loaded.clone();

        log::trace!("staging transaction candidate");
        if let Err(error) = stage_change(&mut candidate, &request.change) {
            return Err(self.abort(
                &candidate,
                original_end,
                original_analyzer_count,
                error.to_string(),
                Vec::new(),
            ));
        }

        log::trace!("analyzing complete transaction candidate");
        let report = match run_analysis(&candidate.lifecore, self.registry) {
        Ok(report) => report,
        Err(error) => {
            return Err(self.abort(
                &candidate,
                original_end,
                original_analyzer_count,
                error.to_string(),
                Vec::new(),
            ));
        }
        };
        let universe = match MessageUniverse::from_analysis(
            &report,
            &candidate.lifecore,
            &candidate.cli.dismissals,
        ) {
        Ok(universe) => universe,
        Err(error) => {
            return Err(self.abort(
                &candidate,
                original_end,
                original_analyzer_count,
                error.to_string(),
                Vec::new(),
            ));
        }
        };
        let mut messages = universe.visible();

        if universe
            .records()
            .iter()
            .any(|message| message.severity == Severity::Inconsistency)
        {
            return Err(self.abort(
                &candidate,
                original_end,
                original_analyzer_count,
                "candidate analysis emitted an Inconsistency",
                messages,
            ));
        }

        if !request.commit {
            log::trace!("transaction analysis passed; returning dry-run candidate");
            return build_result(
                &candidate,
                original_end,
                original_analyzer_count,
                messages,
                TransactionState::DryRun,
            );
        }

        //A Application is delayed until the entire staged graph and configured
        //A analyzer suite have passed. The loaded state remains the rollback
        //A source for every subsequent failure.
        log::trace!("applying analyzed transaction candidate");
        if let Err(error) = self.apply_candidate(&mut candidate) {
            return Err(self.abort(
                &candidate,
                original_end,
                original_analyzer_count,
                error.to_string(),
                messages,
            ));
        }
        if candidate.lifecore.is_mock() {
            return Err(self.abort(
                &candidate,
                original_end,
                original_analyzer_count,
                "applied candidate still contains a Mock circumstance",
                messages,
            ));
        }

        let updates_dismissal = request.dismiss_after_commit.is_some();
        if let Some(digest) = request.dismiss_after_commit {
            insert_dismissal(&mut candidate.cli.dismissals, digest);
        }
        let committed_messages = if updates_dismissal {
            match MessageUniverse::from_analysis(
                &report,
                &candidate.lifecore,
                &candidate.cli.dismissals,
            ) {
            Ok(universe) => universe.visible(),
            Err(error) => {
                return Err(self.abort(
                    &candidate,
                    original_end,
                    original_analyzer_count,
                    error.to_string(),
                    messages,
                ));
            }
            }
        } else {
            messages.clone()
        };

        if let Some(expectation) = request.expected_head.as_ref() {
            log::trace!("verifying canonical head expectation before save");
            if let Err(error) = verify_expected_head(self.store, expectation) {
                return Err(self.abort(
                    &candidate,
                    original_end,
                    original_analyzer_count,
                    error.to_string(),
                    messages,
                ));
            }
        }

        log::trace!("saving applied transaction candidate");
        if let Err(error) = self.store.save(&candidate) {
            return Err(self.abort(
                &candidate,
                original_end,
                original_analyzer_count,
                error.to_string(),
                messages,
            ));
        }

        messages = committed_messages;

        build_result(
            &candidate,
            original_end,
            original_analyzer_count,
            messages,
            TransactionState::Committed,
        )
    }

    fn apply_candidate(&self, candidate: &mut PersistedState) -> Result<(), String> {
        #[cfg(test)]
        if self.fail_apply {
            //A Core apply failures are environment-dependent, so this seam
            //A exercises the transaction rollback edge deterministically.
            return Err("apply failed: injected failure".to_string());
        }

        candidate
            .lifecore
            .apply()
            .map_err(|error| format!("apply failed: {error}"))
    }

    #[cfg(test)]
    pub(crate) fn failing_apply_for_test(
        store: &'a StateStore,
        registry: &'a AnalyzerRegistry,
    ) -> Self {
        Self {
            store,
            registry,
            fail_apply: true,
        }
    }

    fn abort(
        &self,
        candidate: &PersistedState,
        original_end: usize,
        original_analyzer_count: usize,
        message: impl Into<String>,
        messages: Vec<MessageRecord>,
    ) -> AppError {
        let (entries, view) = candidate_records(candidate, original_end)
            .unwrap_or_else(|_| (Vec::new(), ViewSummary::empty()));
        let analyzers = candidate_analyzers(candidate, original_analyzer_count);

        AppError::transaction_aborted(message, entries, analyzers, messages, view)
    }
}

pub fn stage_change(
    candidate: &mut PersistedState,
    change: &PlannedChange,
) -> Result<(), AppError> {
    match change {
    PlannedChange::AddNode {
        type_name,
        body,
        parent,
    } => {
        let parent = parent
            .as_ref()
            .map(|index| pointed_index(&candidate.lifecore, index))
            .transpose()?;
        candidate
            .lifecore
            .add_mock(type_name, body.clone(), parent)
            .map_err(|error| AppError::transaction_stage(error.to_string()))?;
    }
    PlannedChange::AddEdge {
        from,
        type_name,
        to,
    } => {
        let from = pointed_index(&candidate.lifecore, from)?;
        let to = pointed_index(&candidate.lifecore, to)?;
        candidate
            .lifecore
            .connect_mock(from, type_name, to)
            .map_err(|error| AppError::transaction_stage(error.to_string()))?;
    }
    PlannedChange::AddAnalyzer { name, config } => {
        candidate.lifecore.add_analyzer(name, config);
        candidate.lifecore.basic_validation().map_err(|error| {
            AppError::transaction_stage(format!("basic validation failed: {error:?}"))
        })?;
    }
    PlannedChange::Composite(changes) => {
        for change in changes {
            stage_change(candidate, change)?;
        }
    }
    }

    Ok(())
}

pub fn verify_expected_head(
    store: &StateStore,
    expectation: &ExpectedHead,
) -> Result<(), AppError> {
    //A This reread is deliberately adjacent to save. It is an optimistic
    //A compare-before-save guard, not a file lock.
    let current = store.load()?;
    let actual_index = current.lifecore.last().map(|entry| entry.entry().0);

    match (expectation, actual_index) {
    (ExpectedHead::Empty, None) => Ok(()),
    (ExpectedHead::Empty, Some(index)) => {
        let actual = resolve_index(
            &current.lifecore,
            &current.cli,
            index,
            EntryExpectation::Any,
            None,
        )?
        .display_locator();
        Err(AppError::expected_head_mismatch("EMPTY", actual))
    }
    (ExpectedHead::Entry(expected), None) => Err(AppError::expected_head_mismatch(
        expected.to_string(),
        "EMPTY",
    )),
    (ExpectedHead::Entry(expected), Some(index)) => {
        let actual = resolve_index(
            &current.lifecore,
            &current.cli,
            index,
            EntryExpectation::Any,
            None,
        )?;
        if expected.index() == index && actual.fingerprint.as_str().starts_with(expected.prefix()) {
            Ok(())
        } else {
            Err(AppError::expected_head_mismatch(
                expected.to_string(),
                actual.display_locator(),
            ))
        }
    }
    }
}

fn pointed_index(
    graph: &lifecore::LifeCoreList,
    index: &EntryIndex,
) -> Result<lifecore::EntryContext, AppError> {
    let Some(last) = graph.last() else {
        return Err(AppError::transaction_stage(format!(
            "entry index {} does not exist in the candidate",
            index.0
        )));
    };
    let pointed = last.window().look_at(index.absolute());
    if graph.get(pointed).is_none() {
        return Err(AppError::transaction_stage(format!(
            "entry index {} does not exist in the candidate",
            index.0
        )));
    }

    Ok(pointed)
}

fn insert_dismissal(dismissals: &mut Vec<DismissalRecord>, digest: String) {
    match dismissals.binary_search_by(|record| record.digest.cmp(&digest)) {
    Ok(_) => {}
    Err(index) => dismissals.insert(
        index,
        DismissalRecord {
            version: MESSAGE_ID_VERSION,
            digest,
        },
    ),
    }
}

fn graph_end(state: &PersistedState) -> usize {
    state
        .lifecore
        .last()
        .map_or(0, |entry| entry.entry().0.saturating_add(1))
}

fn candidate_records(
    state: &PersistedState,
    original_end: usize,
) -> Result<(Vec<EntryRecord>, ViewSummary), AppError> {
    let view = GraphView::latest(&state.lifecore, &state.cli)?;
    let entries = (original_end..view.end_exclusive())
        .map(|index| {
            resolve_index(
                &state.lifecore,
                &state.cli,
                index,
                EntryExpectation::Any,
                Some(view.end_exclusive()),
            )
            .and_then(|resolved| entry_record(&state.lifecore, &state.cli, &view, &resolved))
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok((entries, view.summary()))
}

fn build_result(
    state: &PersistedState,
    original_end: usize,
    original_analyzer_count: usize,
    messages: Vec<MessageRecord>,
    transaction_state: TransactionState,
) -> Result<TransactionResult, AppError> {
    let (entries, view) = candidate_records(state, original_end)?;
    let analyzers = candidate_analyzers(state, original_analyzer_count);

    Ok(TransactionResult {
        state: transaction_state,
        entries,
        analyzers,
        messages,
        view,
    })
}

fn candidate_analyzers(state: &PersistedState, original_count: usize) -> Vec<AnalyzerRecord> {
    state
        .lifecore
        .current_analysis_suite()
        .0
        .into_iter()
        .skip(original_count)
        .map(|config| AnalyzerRecord {
            name: config.name,
            config: config.config,
        })
        .collect()
}
