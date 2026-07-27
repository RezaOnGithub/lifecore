use lifecore::{Context, EntryIndex, LifeCore, LifeCoreAccess, LifeCoreList};

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
    canonical_end: usize,
}

impl GraphView {
    pub fn construct(
        graph: &LifeCoreList,
        state: &CliStateV1,
        spec: ViewSpec,
    ) -> Result<Self, AppError> {
        let canonical_end = graph
            .last()
            .map_or(0, |entry| entry.index().0.saturating_add(1));
        Self::construct_effective(graph, state, spec, canonical_end)
    }

    pub fn construct_effective(
        graph: &LifeCoreList,
        state: &CliStateV1,
        spec: ViewSpec,
        canonical_end: usize,
    ) -> Result<Self, AppError> {
        match spec {
        ViewSpec::Latest => Self::latest_effective(graph, state, canonical_end),
        ViewSpec::Through(locator) => {
            Self::through_effective(graph, state, &locator, canonical_end)
        }
        }
    }

    pub fn latest(graph: &LifeCoreList, state: &CliStateV1) -> Result<Self, AppError> {
        let canonical_end = graph
            .last()
            .map_or(0, |entry| entry.index().0.saturating_add(1));
        Self::latest_effective(graph, state, canonical_end)
    }

    fn latest_effective(
        graph: &LifeCoreList,
        state: &CliStateV1,
        canonical_end: usize,
    ) -> Result<Self, AppError> {
        let Some(last) = graph.last() else {
            return Ok(Self {
                context: EntryIndex(0).before(),
                end_exclusive: 0,
                boundary: None,
                canonical_end: 0,
            });
        };
        let entry = graph
            .get(last)
            .ok_or_else(|| AppError::invalid_state("graph last entry could not be loaded"))?;
        let fingerprint = crate::locator::EntryFingerprint::for_entry(state, last.index(), &entry)?;
        let boundary = resolve_entry(
            graph,
            state,
            &EntryLocator::parse(&format!("{}:{}", last.index().0, fingerprint.as_str()))?,
            EntryExpectation::Any,
            None,
        )?;

        Ok(Self {
            context: last.context(),
            end_exclusive: last.index().0.saturating_add(1),
            boundary: Some(boundary),
            canonical_end: canonical_end.min(last.index().0.saturating_add(1)),
        })
    }

    pub fn through(
        graph: &LifeCoreList,
        state: &CliStateV1,
        locator: &EntryLocator,
    ) -> Result<Self, AppError> {
        let canonical_end = graph
            .last()
            .map_or(0, |entry| entry.index().0.saturating_add(1));
        Self::through_effective(graph, state, locator, canonical_end)
    }

    fn through_effective(
        graph: &LifeCoreList,
        state: &CliStateV1,
        locator: &EntryLocator,
        canonical_end: usize,
    ) -> Result<Self, AppError> {
        //A The boundary locator is verified against the unbounded canonical
        //A graph before its index is allowed to construct a bounded context.
        let boundary = resolve_entry(graph, state, locator, EntryExpectation::Any, None)?;
        let end_exclusive = boundary.index().saturating_add(1);
        let context = EntryIndex(boundary.index())
            .before_and_including()
            .context();

        Ok(Self {
            context,
            end_exclusive,
            boundary: Some(boundary),
            canonical_end: canonical_end.min(end_exclusive),
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
            canonical_end: self.canonical_end,
            through: self.boundary.as_ref().map(ResolvedEntry::full_locator),
            display_through: self.boundary.as_ref().map(ResolvedEntry::display_locator),
        }
    }
}
