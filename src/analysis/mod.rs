use crate::{DiagnosticLocation, EntryId, LifeCoreError, LifeCoreList, View, ViewId};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt::Debug,
    hash::{DefaultHasher, Hash, Hasher},
};

mod identity;
pub mod presentation;
pub mod suggestion;
pub use identity::{AnalyzerCertificate, AnalyzerIdentity, EvaluationInputs, FindingKey};
pub use suggestion::SuggestedAction;

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

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub analyzer_certificate: AnalyzerCertificate,
    pub subjects: BTreeSet<EntryId>,
    //A Setup has no graph location. Replay can describe an earlier transition,
    //A but never a location outside the invocation prefix.
    pub offending_event: Option<DiagnosticLocation>,
    pub relevance: BTreeSet<EntryId>,
    pub message: String,
    pub kind: MessageKind,
    //A Sticky advisories stay visible at later prefixes until dismissed or filtered.
    //A They have no automatic expiry and do not assert current applicability.
    pub sticky: bool,
}

impl Message {
    pub fn ty(&self) -> MessageType {
        self.kind.ty()
    }
}

impl MessageKind {
    pub fn ty(&self) -> MessageType {
        match self {
        Self::Inconsistency => MessageType::Inconsistency,
        Self::Warning => MessageType::Warning,
        Self::Suggestion(_) => MessageType::Suggestion,
        }
    }
}

pub trait InnerMessageType: Clone + Debug + PartialEq + Eq + Hash {
    fn ty(&self) -> MessageType;
}

//A Keep structured domain findings until analyzer-owned conversion. Tests can
//A assert their variants and details independently of presentation wording.
pub trait TypedAnalyzer: Send + Sync {
    type MSG: InnerMessageType;
    fn version(&self) -> Version;
    fn initialize_inner(&mut self, inputs: &EvaluationInputs) -> Result<Vec<Self::MSG>, String>;
    fn analyze_inner(
        &self,
        view: View<'_>,
        inputs: &EvaluationInputs,
    ) -> Result<Vec<Self::MSG>, String>;
    fn convert_message(&self, message: Self::MSG) -> Message;
}

//A Direct object-safe implementations are supported alongside the typed adapter.
//A Setup receives only frozen inputs; all graph access is through View.
pub trait Analyzer: Send + Sync {
    fn version(&self) -> Version;
    fn initialize(&mut self, inputs: &EvaluationInputs) -> Result<Vec<Message>, String>;
    fn analyze(&self, view: View<'_>, inputs: &EvaluationInputs) -> Result<Vec<Message>, String>;
}

impl<T: TypedAnalyzer> Analyzer for T {
    fn version(&self) -> Version {
        TypedAnalyzer::version(self)
    }
    fn initialize(&mut self, inputs: &EvaluationInputs) -> Result<Vec<Message>, String> {
        Ok(self
            .initialize_inner(inputs)?
            .into_iter()
            .map(|message| self.convert_message(message))
            .collect())
    }
    fn analyze(&self, view: View<'_>, inputs: &EvaluationInputs) -> Result<Vec<Message>, String> {
        Ok(self
            .analyze_inner(view, inputs)?
            .into_iter()
            .map(|message| self.convert_message(message))
            .collect())
    }
}

pub trait AnalyzerFactory {
    fn analyzer_with_config(&mut self, config: &str) -> Result<Box<dyn Analyzer>, LifeCoreError>;
    fn analyzer_name(&self) -> &'static str;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EvaluatedView {
    Initialization,
    Prefix(ViewId),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("analyzer {analyzer:?} failed at {evaluated_view:?}: {message}")]
pub struct ExecutionFailure {
    pub analyzer: AnalyzerIdentity,
    pub inputs: EvaluationInputs,
    pub evaluated_view: EvaluatedView,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub key: FindingKey,
    pub inputs: EvaluationInputs,
    pub evaluated_view: EvaluatedView,
    pub subjects: BTreeSet<EntryId>,
    pub offending_event: Option<DiagnosticLocation>,
    pub relevance: BTreeSet<EntryId>,
    pub message: String,
    pub kind: MessageKind,
    pub sticky: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisOutput {
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
    pub fn run(&mut self, graph: &LifeCoreList) -> Result<AnalysisOutput, LifeCoreError> {
        self.run_view(graph.view(), graph.analysis_suite())
    }
    pub fn run_view(
        &mut self,
        view: View<'_>,
        suite: &SuiteConfig,
    ) -> Result<AnalysisOutput, LifeCoreError> {
        let mut analyzers = vec![];
        let mut identities = vec![];
        for config in &suite.0 {
            let factory = self.factory.get_mut(&config.name).ok_or_else(|| {
                LifeCoreError::AnalyzerNotFound {
                    name: config.name.clone(),
                }
            })?;
            let analyzer = factory.analyzer_with_config(&config.config)?;
            let identity = AnalyzerIdentity {
                name: config.name.clone(),
                version: analyzer.version(),
                configuration: config.config.clone(),
            };
            identities.push(identity);
            analyzers.push(analyzer);
        }
        let mut report = AnalysisOutput {
            view: view.id(),
            inputs: self.inputs.clone(),
            implementations: identities,
            observations: vec![],
        };
        let mut seen = HashMap::new();
        for (analyzer, identity) in analyzers.iter_mut().zip(&report.implementations) {
            let evaluated_view = EvaluatedView::Initialization;
            let output = analyzer.initialize(&self.inputs).map_err(|message| {
                LifeCoreError::AnalyzerExecution(Box::new(ExecutionFailure {
                    analyzer: identity.clone(),
                    inputs: self.inputs.clone(),
                    evaluated_view,
                    message,
                }))
            })?;
            record(
                &mut report.observations,
                &mut seen,
                identity,
                &self.inputs,
                view.prefix(0).expect("empty prefix"),
                evaluated_view,
                output,
            )?;
        }
        //A Preserve suite order and full replay, including the empty prefix.
        //A Deduplication never skips an invocation or the inconsistency gate.
        for (analyzer, identity) in analyzers.iter().zip(&report.implementations) {
            for end_exclusive in 0..=view.len() {
                let prefix = view.prefix(end_exclusive).expect("bounded replay prefix");
                let evaluated_view = EvaluatedView::Prefix(prefix.id());
                let output = analyzer.analyze(prefix, &self.inputs).map_err(|message| {
                    LifeCoreError::AnalyzerExecution(Box::new(ExecutionFailure {
                        analyzer: identity.clone(),
                        inputs: self.inputs.clone(),
                        evaluated_view,
                        message,
                    }))
                })?;
                record(
                    &mut report.observations,
                    &mut seen,
                    identity,
                    &self.inputs,
                    prefix,
                    evaluated_view,
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
    evaluated_view: EvaluatedView,
    output: Vec<Message>,
) -> Result<(), LifeCoreError> {
    for diagnostic in output {
        let invalid = |message: &str| LifeCoreError::InvalidAnalyzerOutput {
            analyzer: identity.name.clone(),
            message: message.into(),
        };
        if let Some(location) = diagnostic.offending_event {
            if evaluated_view == EvaluatedView::Initialization {
                return Err(invalid("initialization cannot have a graph location"));
            }
            let prefix = view
                .prefix(location.view.end_exclusive)
                .map_err(|_| invalid("diagnostic location exceeds invocation"))?;
            if prefix.location(location.entry) != Some(location) {
                return Err(invalid(
                    "diagnostic location is absent or belongs to another history",
                ));
            }
        }
        if diagnostic
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
                analyzer_certificate: diagnostic.analyzer_certificate,
            },
            inputs: inputs.clone(),
            evaluated_view,
            subjects: diagnostic.subjects,
            offending_event: diagnostic.offending_event,
            relevance: diagnostic.relevance,
            message: diagnostic.message,
            kind: diagnostic.kind,
            sticky: diagnostic.sticky,
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

impl AnalysisOutput {
    pub fn observations(&self) -> impl Iterator<Item = &Observation> {
        self.observations.iter()
    }
    pub fn has_error(&self) -> bool {
        self.errors().next().is_some()
    }
    pub fn errors(&self) -> impl Iterator<Item = &Observation> {
        self.observations()
            .filter(|o| o.kind.ty() == MessageType::Inconsistency)
    }
    pub fn warnings(&self) -> impl Iterator<Item = &Observation> {
        self.observations()
            .filter(|o| o.kind.ty() == MessageType::Warning)
    }
    pub fn suggestions(&self) -> impl Iterator<Item = &Observation> {
        self.observations()
            .filter(|o| o.kind.ty() == MessageType::Suggestion)
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
