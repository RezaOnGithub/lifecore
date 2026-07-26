use std::hash::Hash;

use lifecore::{
    Context, LifeCore, LifeCoreError, LifeCoreList,
    analysis::{
        AnalysisRunner, Analyzer, AnalyzerFactory, Config, InnerMessageType, InnerOutput, Message,
        MessageType, Output, SuiteConfig,
    },
};
use lifecore_typecheck::TypeCheckFactory;

use crate::error::AppError;

pub const INTERACTIVE_ANALYZER_NAME: &str = "Interactive";
pub const TYPE_CHECK_ANALYZER_NAME: &str = "Type_Check";

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum InteractiveMessage {}

impl InnerMessageType for InteractiveMessage {
    fn ty(&self) -> MessageType {
        match *self {}
    }
}

#[derive(Clone)]
struct InteractiveOutput(Vec<InteractiveMessage>);

impl InnerOutput<InteractiveMessage> for InteractiveOutput {
    fn push(&mut self, value: InteractiveMessage) {
        self.0.push(value);
    }
}

struct InteractiveAnalyzer;

impl Analyzer<LifeCoreList> for InteractiveAnalyzer {
    type MSG = InteractiveMessage;
    type OUTPUT = InteractiveOutput;

    fn initialize_inner(&mut self, _lifecore: &LifeCoreList) -> Self::OUTPUT {
        InteractiveOutput(Vec::new())
    }

    fn analyze_inner(&self, _lifecore: &LifeCoreList, _window: Context) -> Self::OUTPUT {
        InteractiveOutput(Vec::new())
    }

    fn convert_report(&self, report: Self::OUTPUT) -> Output {
        //A TODO: This assertion documents the work-in-progress no-op analyzer
        //A and should be removed with the placeholder implementation.
        debug_assert!(report.0.is_empty());
        Output(Vec::new())
    }

    fn initialize(&mut self, lifecore: &LifeCoreList) -> Output {
        let report = self.initialize_inner(lifecore);
        self.convert_report(report)
    }

    fn analyze(&self, lifecore: &LifeCoreList, window: Context) -> Output {
        self.convert_report(self.analyze_inner(lifecore, window))
    }
}

#[derive(Default)]
struct InteractiveFactory;

impl AnalyzerFactory<LifeCoreList> for InteractiveFactory {
    fn analyzer_with_config(
        &mut self,
        _config: &str,
    ) -> Result<Box<dyn Analyzer<LifeCoreList>>, LifeCoreError> {
        Ok(Box::new(InteractiveAnalyzer))
    }

    fn analyzer_name(&self) -> &'static str {
        INTERACTIVE_ANALYZER_NAME
    }
}

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
    pub entry_count: usize,
}

pub fn initial_graph() -> LifeCoreList {
    let mut lifecore = LifeCoreList::default();
    lifecore.add_analyzer(TYPE_CHECK_ANALYZER_NAME, "");
    lifecore.add_analyzer(INTERACTIVE_ANALYZER_NAME, "");
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
        entry_count: lifecore.last().map_or(0, |entry| entry.entry().0 + 1),
    })
}

pub fn configured_analyzers(report: &AnalysisReport) -> &[Config] {
    &report.suite.0
}
