use crate::{EntryId, LifeCoreError, LifeCoreList, View, ViewId, transaction::EntryChange};
use semver::Version;
use serde::{Deserialize, Serialize, Serializer, ser::SerializeStruct};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt::Debug,
    hash::{BuildHasher, BuildHasherDefault, DefaultHasher, Hash},
};

mod identity;
pub mod presentation;
pub mod suggestion;
pub use identity::{AnalyzerCertificate, AnalyzerIdentity, EvaluationInputs, FindingKey};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Default, Debug)]
pub struct SuiteConfig(pub Vec<Config>);

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub name: String,
    // The verbatim configuration string, as in `AnalyzerIdentity`.
    pub configuration: String,
}

// A finding's category, serialized in lowercase like every tag in the shared
// vocabulary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    Inconsistency,
    Warning,
    Suggestion,
}

// Serialized only inside `Diagnostic`, flattened into its `kind` and `actions`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum MessageKind {
    Inconsistency,
    Warning,
    Suggestion(Vec<EntryChange>),
}

// What an analyzer says about one finding at one prefix, apart from its
// identity. Every subject and the focus must be visible in that prefix;
// initialization has neither. Serialized flat and tagged by its category in
// `kind`, the word its finding key uses, with a suggestion's ordered
// `actions` beside it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize)]
#[serde(try_from = "StoredDiagnostic")]
pub struct Diagnostic {
    pub subjects: BTreeSet<EntryId>,
    // The entry a reader should look at first. Keep it stable while the finding
    // holds, so re-emissions at consecutive prefixes merge into one range.
    pub focus: Option<EntryId>,
    pub message: String,
    pub kind: MessageKind,
    // Sticky advisories stay visible at later prefixes until dismissed or filtered.
    // They have no automatic expiry and do not assert current applicability.
    pub sticky: bool,
}

impl Serialize for Diagnostic {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let actions = match &self.kind {
        MessageKind::Suggestion(actions) => Some(actions),
        MessageKind::Inconsistency | MessageKind::Warning => None,
        };
        let mut state =
            serializer.serialize_struct("Diagnostic", 5 + usize::from(actions.is_some()))?;
        state.serialize_field("kind", &self.kind.ty())?;
        if let Some(actions) = actions {
            state.serialize_field("actions", actions)?;
        }
        state.serialize_field("subjects", &self.subjects)?;
        state.serialize_field("focus", &self.focus)?;
        state.serialize_field("message", &self.message)?;
        state.serialize_field("sticky", &self.sticky)?;
        state.end()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredDiagnostic {
    kind: MessageType,
    #[serde(default, deserialize_with = "crate::entries::present")]
    actions: Option<Vec<EntryChange>>,
    subjects: BTreeSet<EntryId>,
    focus: Option<EntryId>,
    message: String,
    sticky: bool,
}

// A suggestion always carries `actions`, possibly empty; nothing else may.
impl TryFrom<StoredDiagnostic> for Diagnostic {
    type Error = &'static str;
    fn try_from(stored: StoredDiagnostic) -> Result<Self, Self::Error> {
        let kind = match (stored.kind, stored.actions) {
        (MessageType::Suggestion, Some(actions)) => MessageKind::Suggestion(actions),
        (MessageType::Suggestion, None) => return Err("a suggestion requires `actions`"),
        (_, Some(_)) => return Err("only a suggestion carries `actions`"),
        (MessageType::Inconsistency, None) => MessageKind::Inconsistency,
        (MessageType::Warning, None) => MessageKind::Warning,
        };
        Ok(Self {
            subjects: stored.subjects,
            focus: stored.focus,
            message: stored.message,
            kind,
            sticky: stored.sticky,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Message {
    pub analyzer_certificate: AnalyzerCertificate,
    pub diagnostic: Diagnostic,
}

impl Message {
    pub fn ty(&self) -> MessageType {
        self.diagnostic.kind.ty()
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

// Keep structured domain findings until analyzer-owned conversion. Tests can
// assert their variants and details independently of presentation wording.
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

// Direct object-safe implementations are supported alongside the typed adapter.
// Setup receives only frozen inputs; all graph access is through View.
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
    fn analyzer_with_config(
        &mut self,
        configuration: &str,
    ) -> Result<Box<dyn Analyzer>, LifeCoreError>;
    fn analyzer_name(&self) -> &'static str;
}

// `Prefixes` is the contiguous range of replayed prefixes, both ends
// inclusive, at which an analyzer emitted the same diagnostic; `first` equals
// `last` for a single prefix.
// Serialized as `{"kind": "initialization"}` or `{"kind": "prefixes", first,
// last}`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", from = "StoredView")]
pub enum EvaluatedView {
    Initialization,
    Prefixes { first: ViewId, last: ViewId },
}

// Decoding form of `EvaluatedView`: an internally tagged unit variant would
// ignore unknown members, so initialization decodes as an empty struct.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum StoredView {
    Initialization {},
    Prefixes { first: ViewId, last: ViewId },
}

impl From<StoredView> for EvaluatedView {
    fn from(stored: StoredView) -> Self {
        match stored {
        StoredView::Initialization {} => Self::Initialization,
        StoredView::Prefixes { first, last } => Self::Prefixes { first, last },
        }
    }
}

impl EvaluatedView {
    pub(crate) fn prefix(view: ViewId) -> Self {
        Self::Prefixes {
            first: view,
            last: view,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, thiserror::Error)]
#[error("analyzer {analyzer:?} failed at {evaluated_view:?}: {message}")]
pub struct ExecutionFailure {
    pub analyzer: AnalyzerIdentity,
    pub inputs: EvaluationInputs,
    pub evaluated_view: EvaluatedView,
    pub message: String,
}

// The evaluation inputs are those of the enclosing `AnalysisOutput`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub key: FindingKey,
    pub evaluated_view: EvaluatedView,
    pub diagnostic: Diagnostic,
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
        self.run_view_with_hasher(view, suite, BuildHasherDefault::<DefaultHasher>::default())
    }
    // Tests supply a colliding hasher to exercise complete-value comparison.
    fn run_view_with_hasher<S: BuildHasher>(
        &mut self,
        view: View<'_>,
        suite: &SuiteConfig,
        hasher: S,
    ) -> Result<AnalysisOutput, LifeCoreError> {
        let mut analyzers = vec![];
        let mut identities = vec![];
        for config in &suite.0 {
            let factory = self.factory.get_mut(&config.name).ok_or_else(|| {
                LifeCoreError::AnalyzerNotFound {
                    name: config.name.clone(),
                }
            })?;
            let analyzer = factory.analyzer_with_config(&config.configuration)?;
            let identity = AnalyzerIdentity {
                name: config.name.clone(),
                version: analyzer.version(),
                configuration: config.configuration.clone(),
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
        let mut recorder = Recorder {
            observations: vec![],
            index: HashMap::new(),
            hasher,
        };
        for (analyzer, identity) in analyzers.iter_mut().zip(&report.implementations) {
            let output = analyzer.initialize(&self.inputs).map_err(|message| {
                LifeCoreError::AnalyzerExecution(Box::new(ExecutionFailure {
                    analyzer: identity.clone(),
                    inputs: self.inputs.clone(),
                    evaluated_view: EvaluatedView::Initialization,
                    message,
                }))
            })?;
            recorder.record(identity, view.id().database, None, output)?;
        }
        // Preserve suite order and full replay, including the empty prefix.
        // Deduplication never skips an invocation or the inconsistency gate.
        for (analyzer, identity) in analyzers.iter().zip(&report.implementations) {
            for end_exclusive in 0..=view.len() {
                let prefix = view.prefix(end_exclusive).expect("bounded replay prefix");
                let output = analyzer.analyze(prefix, &self.inputs).map_err(|message| {
                    LifeCoreError::AnalyzerExecution(Box::new(ExecutionFailure {
                        analyzer: identity.clone(),
                        inputs: self.inputs.clone(),
                        evaluated_view: EvaluatedView::prefix(prefix.id()),
                        message,
                    }))
                })?;
                recorder.record(identity, view.id().database, Some(prefix), output)?;
            }
        }
        report.observations = recorder.observations;
        Ok(report)
    }
}

// Collects observations in emission order. A diagnostic re-emitted at the
// next prefix extends the range of the observation with the same key and
// diagnostic instead of adding another, so a finding that holds from its
// append to the head is one observation rather than one per prefix.
struct Recorder<S> {
    observations: Vec<Observation>,
    // Indices by a hash of (key, diagnostic) rather than a second owned copy
    // of every diagnostic. A hash collision still compares complete values,
    // both to deduplicate and to choose the range to extend.
    index: HashMap<u64, Vec<usize>>,
    hasher: S,
}

impl<S: BuildHasher> Recorder<S> {
    // `prefix` is `None` for initialization.
    fn record(
        &mut self,
        identity: &AnalyzerIdentity,
        database: uuid::Uuid,
        prefix: Option<View<'_>>,
        output: Vec<Message>,
    ) -> Result<(), LifeCoreError> {
        for message in output {
            let invalid = |message: &str| LifeCoreError::InvalidAnalyzerOutput {
                analyzer: identity.name.clone(),
                message: message.into(),
            };
            let diagnostic = &message.diagnostic;
            match prefix {
            None if diagnostic.focus.is_some() || !diagnostic.subjects.is_empty() => {
                return Err(invalid("initialization cannot name a focus or subjects"));
            }
            Some(view)
                if diagnostic
                    .subjects
                    .iter()
                    .chain(&diagnostic.focus)
                    .any(|id| view.get(*id).is_none()) =>
            {
                return Err(invalid(
                    "finding subject or focus is outside the evaluated prefix",
                ));
            }
            _ => {}
            }
            let key = FindingKey {
                database,
                analyzer: identity.clone(),
                kind: message.ty(),
                analyzer_certificate: message.analyzer_certificate,
            };
            self.insert(key, message.diagnostic, prefix.map(View::id));
        }
        Ok(())
    }

    fn insert(&mut self, key: FindingKey, diagnostic: Diagnostic, prefix: Option<ViewId>) {
        let hash = self.hasher.hash_one((&key, &diagnostic));
        let positions = self.index.entry(hash).or_default();
        let same = positions
            .iter()
            .copied()
            .filter(|&i| {
                let observation = &self.observations[i];
                observation.key == key && observation.diagnostic == diagnostic
            })
            .collect::<Vec<_>>();
        let evaluated_view = match prefix {
        None => {
            if same
                .iter()
                .any(|&i| self.observations[i].evaluated_view == EvaluatedView::Initialization)
            {
                return;
            }
            EvaluatedView::Initialization
        }
        Some(at) => {
            let mut adjacent = None;
            for &i in &same {
                let EvaluatedView::Prefixes { first, last } = self.observations[i].evaluated_view
                else {
                    continue;
                };
                // A duplicated suite entry replays prefixes already covered.
                if (first.end_exclusive..=last.end_exclusive).contains(&at.end_exclusive) {
                    return;
                }
                if last.end_exclusive + 1 == at.end_exclusive {
                    adjacent = Some(i);
                }
            }
            if let Some(i) = adjacent {
                if let EvaluatedView::Prefixes { last, .. } =
                    &mut self.observations[i].evaluated_view
                {
                    *last = at;
                }
                return;
            }
            EvaluatedView::prefix(at)
        }
        };
        positions.push(self.observations.len());
        self.observations.push(Observation {
            key,
            evaluated_view,
            diagnostic,
        });
    }
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
            .filter(|o| o.diagnostic.kind.ty() == MessageType::Inconsistency)
    }
    pub fn warnings(&self) -> impl Iterator<Item = &Observation> {
        self.observations()
            .filter(|o| o.diagnostic.kind.ty() == MessageType::Warning)
    }
    pub fn suggestions(&self) -> impl Iterator<Item = &Observation> {
        self.observations()
            .filter(|o| o.diagnostic.kind.ty() == MessageType::Suggestion)
    }
}

#[cfg(test)]
mod test;
