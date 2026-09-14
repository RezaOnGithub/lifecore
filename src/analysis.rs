use crate::{DiagnosticLocation, EntryId, LifeCoreError, LifeCoreList, View, ViewId};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt::Debug,
    hash::{DefaultHasher, Hash, Hasher},
};

mod identity;
pub mod presentation;
pub mod repair;
pub use identity::{AnalyzerIdentity, EvaluationInputs, Fact, Finding, FindingKey};
pub use repair::{RevisionIntent, SuggestedAction};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Default, Debug)]
pub struct SuiteConfig(pub Vec<Config>);

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct Config {
    pub name: String,
    pub config: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MessageType {
    Inconsistency,
    Warning,
    Suggestion,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MessageKind {
    Inconsistency,
    Warning,
    Suggestion(Vec<SuggestedAction>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ObservationScope {
    State,
    //A Event retention makes no claim about current applicability or resolution.
    Event,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub finding: Finding,
    //A Setup has no graph location. Replay can describe an earlier transition,
    //A but never a location outside the invocation prefix.
    pub offending_event: Option<DiagnosticLocation>,
    pub relevance: BTreeSet<EntryId>,
    pub message: String,
    pub kind: MessageKind,
    pub scope: ObservationScope,
}

impl Message {
    pub fn ty(&self) -> MessageType {
        match self.kind {
        MessageKind::Inconsistency => MessageType::Inconsistency,
        MessageKind::Warning => MessageType::Warning,
        MessageKind::Suggestion(_) => MessageType::Suggestion,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Output(pub Vec<Message>);

pub trait InnerMessageType: Clone + Debug + PartialEq + Eq + Hash {
    fn ty(&self) -> MessageType;
}
pub trait InnerOutput<MSG: InnerMessageType>: Clone {
    fn push(&mut self, v: MSG);
}

pub trait TypedAnalyzer: Send + Sync {
    type MSG: InnerMessageType;
    type OUTPUT: InnerOutput<Self::MSG>;
    fn identity(&self) -> AnalyzerIdentity;
    fn initialize_inner(&mut self, inputs: &EvaluationInputs) -> Result<Self::OUTPUT, String>;
    fn analyze_inner(
        &self,
        view: View<'_>,
        inputs: &EvaluationInputs,
    ) -> Result<Self::OUTPUT, String>;
    fn convert_report(&self, report: Self::OUTPUT) -> Output;
}

//A Direct object-safe implementations are supported alongside the typed adapter.
//A Setup receives only frozen inputs; all graph access is through View.
pub trait Analyzer: Send + Sync {
    fn identity(&self) -> AnalyzerIdentity;
    fn initialize(&mut self, inputs: &EvaluationInputs) -> Result<Output, String>;
    fn analyze(&self, view: View<'_>, inputs: &EvaluationInputs) -> Result<Output, String>;
}

impl<T: TypedAnalyzer> Analyzer for T {
    fn identity(&self) -> AnalyzerIdentity {
        TypedAnalyzer::identity(self)
    }
    fn initialize(&mut self, inputs: &EvaluationInputs) -> Result<Output, String> {
        let report = self.initialize_inner(inputs)?;
        Ok(self.convert_report(report))
    }
    fn analyze(&self, view: View<'_>, inputs: &EvaluationInputs) -> Result<Output, String> {
        Ok(self.convert_report(self.analyze_inner(view, inputs)?))
    }
}

pub trait AnalyzerFactory {
    fn analyzer_with_config(&mut self, config: &str) -> Result<Box<dyn Analyzer>, LifeCoreError>;
    fn analyzer_name(&self) -> &'static str;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Invocation {
    Initialization,
    Prefix(ViewId),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("analyzer {analyzer:?} failed at {invocation:?}: {message}")]
pub struct ExecutionFailure {
    pub analyzer: AnalyzerIdentity,
    pub inputs: EvaluationInputs,
    pub invocation: Invocation,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub key: FindingKey,
    pub inputs: EvaluationInputs,
    pub invocation: Invocation,
    pub diagnostic: Message,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub view: ViewId,
    pub inputs: EvaluationInputs,
    pub implementations: Vec<AnalyzerIdentity>,
    observations: Vec<Observation>,
}

#[derive(Default)]
pub struct AnalysisRunner {
    factory: BTreeMap<String, Box<dyn AnalyzerFactory>>,
    inputs: EvaluationInputs,
}

impl AnalysisRunner {
    pub fn new(inputs: EvaluationInputs) -> Self {
        Self {
            inputs,
            ..Self::default()
        }
    }
    pub fn inputs(&self) -> &EvaluationInputs {
        &self.inputs
    }
    pub fn register_analyzer_kind(
        &mut self,
        factory: Box<dyn AnalyzerFactory>,
    ) -> Result<(), LifeCoreError> {
        use std::collections::btree_map::Entry;
        match self.factory.entry(factory.analyzer_name().into()) {
        Entry::Vacant(slot) => {
            slot.insert(factory);
            Ok(())
        }
        Entry::Occupied(slot) => Err(LifeCoreError::DuplicateAnalyzer {
            dupe_name: slot.key().clone(),
        }),
        }
    }
    pub fn run(&mut self, graph: &LifeCoreList) -> Result<Report, LifeCoreError> {
        self.run_view(graph.view(), graph.analysis_suite())
    }
    pub fn run_view(
        &mut self,
        view: View<'_>,
        suite: &SuiteConfig,
    ) -> Result<Report, LifeCoreError> {
        let mut analyzers = vec![];
        let mut identities = vec![];
        for config in &suite.0 {
            let factory = self.factory.get_mut(&config.name).ok_or_else(|| {
                LifeCoreError::AnalyzerNotFound {
                    name: config.name.clone(),
                }
            })?;
            let analyzer = factory.analyzer_with_config(&config.config)?;
            let identity = analyzer.identity();
            if identity.name != config.name || identity.version.is_empty() {
                return Err(LifeCoreError::InvalidAnalyzerOutput {
                    analyzer: config.name.clone(),
                    message: "implementation identity must match registration and have a version"
                        .into(),
                });
            }
            identities.push(identity);
            analyzers.push(analyzer);
        }
        let mut report = Report {
            view: view.id(),
            inputs: self.inputs.clone(),
            implementations: identities,
            observations: vec![],
        };
        let mut seen = HashMap::new();
        for (analyzer, identity) in analyzers.iter_mut().zip(&report.implementations) {
            let invocation = Invocation::Initialization;
            let output = analyzer.initialize(&self.inputs).map_err(|message| {
                LifeCoreError::AnalyzerExecution(Box::new(ExecutionFailure {
                    analyzer: identity.clone(),
                    inputs: self.inputs.clone(),
                    invocation,
                    message,
                }))
            })?;
            record(
                &mut report.observations,
                &mut seen,
                identity,
                &self.inputs,
                view.prefix(0).expect("empty prefix"),
                invocation,
                output,
            )?;
        }
        //A Preserve suite order and full replay, including the empty prefix.
        //A Deduplication never skips an invocation or the inconsistency gate.
        for (analyzer, identity) in analyzers.iter().zip(&report.implementations) {
            for end in 0..=view.len() {
                let prefix = view.prefix(end).expect("bounded replay prefix");
                let invocation = Invocation::Prefix(prefix.id());
                let output = analyzer.analyze(prefix, &self.inputs).map_err(|message| {
                    LifeCoreError::AnalyzerExecution(Box::new(ExecutionFailure {
                        analyzer: identity.clone(),
                        inputs: self.inputs.clone(),
                        invocation,
                        message,
                    }))
                })?;
                record(
                    &mut report.observations,
                    &mut seen,
                    identity,
                    &self.inputs,
                    prefix,
                    invocation,
                    output,
                )?;
            }
        }
        Ok(report)
    }
}

fn record(
    observations: &mut Vec<Observation>,
    seen: &mut HashMap<u64, Vec<usize>>,
    identity: &AnalyzerIdentity,
    inputs: &EvaluationInputs,
    view: View<'_>,
    invocation: Invocation,
    output: Output,
) -> Result<(), LifeCoreError> {
    for diagnostic in output.0 {
        let invalid = |message: &str| LifeCoreError::InvalidAnalyzerOutput {
            analyzer: identity.name.clone(),
            message: message.into(),
        };
        if diagnostic.finding.code.is_empty() {
            return Err(invalid("finding code is empty"));
        }
        if let Some(location) = diagnostic.offending_event {
            if invocation == Invocation::Initialization {
                return Err(invalid("initialization cannot have a graph location"));
            }
            let prefix = view
                .prefix(location.view.end)
                .map_err(|_| invalid("diagnostic location exceeds invocation"))?;
            if prefix.location(location.entry) != Some(location) {
                return Err(invalid(
                    "diagnostic location is absent or belongs to another history",
                ));
            }
        }
        if diagnostic
            .finding
            .subjects
            .iter()
            .chain(&diagnostic.relevance)
            .any(|id| view.get(*id).is_none())
        {
            return Err(invalid(
                "finding subject or relevance is outside invocation",
            ));
        }
        let observation = Observation {
            key: FindingKey {
                database: view.id().database,
                analyzer: identity.clone(),
                kind: diagnostic.ty(),
                finding: diagnostic.finding.clone(),
            },
            inputs: inputs.clone(),
            invocation,
            diagnostic,
        };
        //A Keep indices instead of a second owned copy of every diagnostic.
        //A A hash collision still compares the complete observation.
        let mut hasher = DefaultHasher::new();
        observation.hash(&mut hasher);
        let positions = seen.entry(hasher.finish()).or_default();
        if !positions.iter().any(|&i| observations[i] == observation) {
            positions.push(observations.len());
            observations.push(observation);
        }
    }
    Ok(())
}

impl Output {
    pub fn has_error(&self) -> bool {
        self.errors().next().is_some()
    }
    pub fn errors(&self) -> impl Iterator<Item = &Message> {
        self.0
            .iter()
            .filter(|m| m.ty() == MessageType::Inconsistency)
    }
    pub fn warnings(&self) -> impl Iterator<Item = &Message> {
        self.0.iter().filter(|m| m.ty() == MessageType::Warning)
    }
    pub fn suggestions(&self) -> impl Iterator<Item = &Message> {
        self.0.iter().filter(|m| m.ty() == MessageType::Suggestion)
    }
}

impl Report {
    pub fn observations(&self) -> impl Iterator<Item = &Observation> {
        self.observations.iter()
    }
    pub fn has_error(&self) -> bool {
        self.errors().next().is_some()
    }
    pub fn errors(&self) -> impl Iterator<Item = &Observation> {
        self.observations()
            .filter(|o| o.diagnostic.ty() == MessageType::Inconsistency)
    }
    pub fn warnings(&self) -> impl Iterator<Item = &Observation> {
        self.observations()
            .filter(|o| o.diagnostic.ty() == MessageType::Warning)
    }
    pub fn suggestions(&self) -> impl Iterator<Item = &Observation> {
        self.observations()
            .filter(|o| o.diagnostic.ty() == MessageType::Suggestion)
    }
    pub fn into_result(self) -> Result<Self, LifeCoreError> {
        if self.has_error() {
            Err(LifeCoreError::SystemsError(
                self.errors().cloned().collect(),
            ))
        } else {
            Ok(self)
        }
    }
}

#[cfg(test)]
mod test;
