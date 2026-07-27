use std::{collections::BTreeSet, path::PathBuf};

use lifecore::LifeCore;

use crate::{
    analysis::{AnalyzerRegistry, run_analysis},
    error::AppError,
    messages::{MessageUniverse, Severity},
    sidecar::{SidecarDumpItem, SidecarPlan, build_sidecar_plan, parse_sidecar, read_sidecar},
    state::PersistedState,
    view::{GraphView, ViewSpec},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EffectiveCapability {
    Browse,
    Check,
    Dump,
    Commit,
}

#[derive(Clone)]
pub struct EffectiveGraph {
    state: PersistedState,
    canonical_end: usize,
    plan: Option<SidecarPlan>,
    temporary_aliases: BTreeSet<String>,
    prepared_messages: Option<MessageUniverse>,
}

impl EffectiveGraph {
    pub fn state(&self) -> &PersistedState {
        &self.state
    }

    pub fn canonical_end(&self) -> usize {
        self.canonical_end
    }

    pub fn is_attached(&self) -> bool {
        self.plan.is_some()
    }

    pub fn plan(&self) -> Option<&SidecarPlan> {
        self.plan.as_ref()
    }

    pub fn is_temporary_alias(&self, name: &str) -> bool {
        self.temporary_aliases.contains(name)
    }

    pub fn view(&self, spec: ViewSpec) -> Result<GraphView, AppError> {
        GraphView::construct_effective(
            &self.state.lifecore,
            &self.state.cli,
            spec,
            self.canonical_end,
        )
    }

    pub fn messages(&self, registry: &AnalyzerRegistry) -> Result<MessageUniverse, AppError> {
        if let Some(universe) = self.prepared_messages.as_ref() {
            return Ok(universe.clone());
        }

        let report = run_analysis(&self.state.lifecore, registry)?;
        MessageUniverse::from_analysis(&report, &self.state.lifecore, &self.state.cli.dismissals)
    }

    pub fn entry_record(
        &self,
        view: &GraphView,
        resolved: &crate::locator::ResolvedEntry,
    ) -> Result<crate::app::EntryRecord, AppError> {
        let mut record =
            crate::entry::entry_record(&self.state.lifecore, &self.state.cli, view, resolved)?;
        record.provisional = record.index >= self.canonical_end;
        record.temporary_aliases = record
            .aliases
            .iter()
            .filter(|name| self.is_temporary_alias(name))
            .cloned()
            .collect();

        Ok(record)
    }

    pub fn dump_suffix(&self) -> Result<Vec<SidecarDumpItem>, AppError> {
        if self.plan.is_none() {
            return Err(AppError::sidecar_not_attached());
        }

        let view = self.view(ViewSpec::Latest)?;
        (self.canonical_end..view.end_exclusive())
            .map(|index| {
                let resolved = crate::locator::resolve_index(
                    &self.state.lifecore,
                    &self.state.cli,
                    index,
                    crate::locator::EntryExpectation::Any,
                    Some(view.end_exclusive()),
                )?;
                let entry = self.entry_record(&view, &resolved)?;
                let temporary_aliases = entry.temporary_aliases.clone();

                Ok(SidecarDumpItem {
                    entry,
                    temporary_aliases,
                })
            })
            .collect()
    }
}

#[derive(Clone)]
pub enum EffectiveState {
    EffectiveReady(EffectiveGraph),
    SidecarDiagnosable(EffectiveGraph),
    SidecarUnavailable { path: PathBuf, reason: String },
}

impl EffectiveState {
    pub fn require(
        self,
        capability: EffectiveCapability,
        operation: &str,
    ) -> Result<EffectiveGraph, AppError> {
        match (self, capability) {
        (Self::EffectiveReady(graph), EffectiveCapability::Browse | EffectiveCapability::Check) => {
            Ok(graph)
        }
        (Self::EffectiveReady(graph), EffectiveCapability::Dump | EffectiveCapability::Commit)
            if graph.is_attached() =>
        {
            Ok(graph)
        }
        (Self::EffectiveReady(_), EffectiveCapability::Dump | EffectiveCapability::Commit) => {
            Err(AppError::sidecar_not_attached())
        }
        (Self::SidecarDiagnosable(graph), EffectiveCapability::Check) => Ok(graph),
        (Self::SidecarDiagnosable(_), _) => Err(AppError::sidecar_diagnosable(operation)),
        (Self::SidecarUnavailable { path, reason }, _) => {
            Err(AppError::sidecar_unavailable(operation, &path, reason))
        }
        }
    }
}

pub fn prepare_effective(
    canonical: &PersistedState,
    registry: &AnalyzerRegistry,
) -> EffectiveState {
    let canonical_end = graph_end(canonical);
    let Some(path) = canonical.cli.sidecar_path.as_ref() else {
        return EffectiveState::EffectiveReady(EffectiveGraph {
            state: canonical.clone(),
            canonical_end,
            plan: None,
            temporary_aliases: BTreeSet::new(),
            prepared_messages: None,
        });
    };

    let built = read_sidecar(path)
        .and_then(|bytes| parse_sidecar(&bytes))
        .and_then(|document| build_sidecar_plan(canonical, document));
    let built = match built {
    Ok(built) => built,
    Err(error) => {
        return EffectiveState::SidecarUnavailable {
            path: path.clone(),
            reason: error.to_string(),
        };
    }
    };
    let report = match run_analysis(&built.staged.lifecore, registry) {
    Ok(report) => report,
    Err(error) => {
        return EffectiveState::SidecarUnavailable {
            path: path.clone(),
            reason: error.to_string(),
        };
    }
    };
    let universe = match MessageUniverse::from_analysis(
        &report,
        &built.staged.lifecore,
        &built.staged.cli.dismissals,
    ) {
    Ok(universe) => universe,
    Err(error) => {
        return EffectiveState::SidecarUnavailable {
            path: path.clone(),
            reason: error.to_string(),
        };
    }
    };
    let diagnosable = universe
        .records()
        .iter()
        .any(|message| message.severity == Severity::Inconsistency);
    let graph = EffectiveGraph {
        state: built.staged,
        canonical_end,
        temporary_aliases: built.plan.temporary_aliases.keys().cloned().collect(),
        plan: Some(built.plan),
        prepared_messages: Some(universe),
    };

    if diagnosable {
        EffectiveState::SidecarDiagnosable(graph)
    } else {
        EffectiveState::EffectiveReady(graph)
    }
}

pub fn reject_attached_mutation(state: &PersistedState, operation: &str) -> Result<(), AppError> {
    if state.cli.sidecar_path.is_some() {
        return Err(AppError::mutation_with_sidecar(operation));
    }

    Ok(())
}

fn graph_end(state: &PersistedState) -> usize {
    state
        .lifecore
        .last()
        .map_or(0, |entry| entry.index().0.saturating_add(1))
}
