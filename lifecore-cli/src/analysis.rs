use lifecore::{
    LifeCore, LifeCoreList,
    analysis::{AnalysisRunner, AnalyzerFactory, Config, Message, SuiteConfig},
};
use lifecore_typecheck::TypeCheckFactory;

use crate::{error::AppError, interactive_analyzer::InteractiveFactory};

#[derive(Clone, Debug)]
pub struct AnalyzerRegistry {
    type_check: bool,
    interactive: bool,
}

impl AnalyzerRegistry {
    pub fn built_in() -> Self {
        Self {
            type_check: true,
            interactive: true,
        }
    }

    #[cfg(test)]
    pub(crate) fn empty() -> Self {
        Self {
            type_check: false,
            interactive: false,
        }
    }

    fn runner(&self) -> Result<AnalysisRunner<LifeCoreList>, AppError> {
        let mut runner = AnalysisRunner::default();

        if self.type_check {
            runner
                .register_analyzer_kind(Box::new(TypeCheckFactory::<LifeCoreList>::default()))
                .map_err(|error| AppError::analysis(error.to_string()))?;
        }

        if self.interactive {
            runner
                .register_analyzer_kind(Box::new(InteractiveFactory))
                .map_err(|error| AppError::analysis(error.to_string()))?;
        }

        Ok(runner)
    }
}

#[derive(Clone)]
pub struct AnalysisReport {
    pub suite: SuiteConfig,
    pub messages: Vec<Message>,
}

pub fn initial_graph() -> LifeCoreList {
    let mut lifecore = LifeCoreList::default();
    let type_check = TypeCheckFactory::<LifeCoreList>::default();
    let interactive = InteractiveFactory;

    //A Factory names are the persisted analyzer identifiers, so initialization
    //A asks each factory instead of maintaining constants that can drift.
    lifecore.add_analyzer(type_check.analyzer_name(), "");
    lifecore.add_analyzer(interactive.analyzer_name(), "");
    lifecore
}

pub fn run_analysis(
    lifecore: &LifeCoreList,
    registry: &AnalyzerRegistry,
) -> Result<AnalysisReport, AppError> {
    let mut runner = registry.runner()?;
    let output = runner
        .run(lifecore)
        .map_err(|error| AppError::analysis(error.to_string()))?;

    Ok(AnalysisReport {
        suite: lifecore.current_analysis_suite(),
        messages: output.0,
    })
}

pub fn configured_analyzers(report: &AnalysisReport) -> &[Config] {
    &report.suite.0
}
