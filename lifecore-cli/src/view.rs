use lifecore::{Context, EntryIndex, LifeCore, LifeCoreList};

use crate::{
    app::ViewSummary,
    error::AppError,
    locator::{EntryExpectation, EntryLocator, ResolvedEntry, resolve_entry},
    state::CliStateV1,
};

#[derive(Clone, Debug)]
pub enum ViewSpec {
    Latest,
    Through(EntryLocator),
}

#[derive(Clone)]
pub struct GraphView {
    context: Context,
    end_exclusive: usize,
    boundary: Option<ResolvedEntry>,
}

impl GraphView {
    pub fn construct(
        graph: &LifeCoreList,
        state: &CliStateV1,
        spec: ViewSpec,
    ) -> Result<Self, AppError> {
        match spec {
        ViewSpec::Latest => Self::latest(graph, state),
        ViewSpec::Through(locator) => Self::through(graph, state, &locator),
        }
    }

    pub fn latest(graph: &LifeCoreList, state: &CliStateV1) -> Result<Self, AppError> {
        let Some(last) = graph.last() else {
            return Ok(Self {
                context: EntryIndex(0).before(),
                end_exclusive: 0,
                boundary: None,
            });
        };
        let entry = graph
            .get(last)
            .ok_or_else(|| AppError::invalid_state("graph last entry could not be loaded"))?;
        let fingerprint = crate::locator::EntryFingerprint::for_entry(state, last.entry(), &entry)?;
        let boundary = resolve_entry(
            graph,
            state,
            &EntryLocator::parse(&format!("{}:{}", last.entry().0, fingerprint.as_str()))?,
            EntryExpectation::Any,
            None,
        )?;

        Ok(Self {
            context: last.window(),
            end_exclusive: last.entry().0.saturating_add(1),
            boundary: Some(boundary),
        })
    }

    pub fn through(
        graph: &LifeCoreList,
        state: &CliStateV1,
        locator: &EntryLocator,
    ) -> Result<Self, AppError> {
        //A The boundary locator is verified against the unbounded canonical
        //A graph before its index is allowed to construct a bounded context.
        let boundary = resolve_entry(graph, state, locator, EntryExpectation::Any, None)?;
        let context = EntryIndex(boundary.index()).before_and_including().window();

        Ok(Self {
            context,
            end_exclusive: boundary.index().saturating_add(1),
            boundary: Some(boundary),
        })
    }

    pub fn context(&self) -> Context {
        self.context
    }

    pub fn end_exclusive(&self) -> usize {
        self.end_exclusive
    }

    pub fn summary(&self) -> ViewSummary {
        ViewSummary {
            end: self.end_exclusive,
            through: self.boundary.as_ref().map(ResolvedEntry::full_locator),
            display_through: self.boundary.as_ref().map(ResolvedEntry::display_locator),
        }
    }
}
