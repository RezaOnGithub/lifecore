use std::hash::Hash;

use lifecore::{
    Context, LifeCoreError, LifeCoreList,
    analysis::{Analyzer, AnalyzerFactory, InnerMessageType, InnerOutput, MessageType, Output},
};

//A The empty message type makes the Stage 2 no-op explicit: this analyzer
//A cannot accidentally emit a diagnostic before its deferred behavior exists.
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
        //A TODO: Remove this assertion with the placeholder implementation
        //A when the analyzer gains its deferred connection-recreation behavior.
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
pub(crate) struct InteractiveFactory;

impl AnalyzerFactory<LifeCoreList> for InteractiveFactory {
    fn analyzer_with_config(
        &mut self,
        _config: &str,
    ) -> Result<Box<dyn Analyzer<LifeCoreList>>, LifeCoreError> {
        Ok(Box::new(InteractiveAnalyzer))
    }

    fn analyzer_name(&self) -> &'static str {
        "Interactive"
    }
}
