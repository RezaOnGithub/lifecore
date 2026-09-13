use crate::{Body, DiagnosticLocation, EntryId, LifeCoreError, LifeCoreList, View};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fmt::Debug, hash::Hash};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Default, Debug)]
pub struct SuiteConfig(pub Vec<Config>);

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
pub struct Config {
    pub name: String,
    pub config: String,
}

#[derive(Hash, PartialEq, Eq, Clone, Debug)]
pub struct Inconsistency {
    pub offending_event: DiagnosticLocation,
    pub system_name: String,
    pub system_config: String,
    pub message: String,
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct Warning {
    pub offending_event: DiagnosticLocation,
    pub system_name: String,
    pub system_config: String,
    pub relevance: Vec<EntryId>,
    pub message: String,
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct Suggestion {
    pub offending_event: DiagnosticLocation,
    pub system_name: String,
    pub system_config: String,
    pub relevance: Vec<EntryId>,
    pub message: String,
    pub action: Vec<SuggestedAction>,
}

#[derive(PartialEq, Eq, Clone, Hash, Debug)]
pub enum SuggestedAction {
    AddNode {
        ty_name: String,
        payload: Body,
        parent: Option<EntryId>,
    },
    AddEdge {
        from: EntryId,
        ty_name: String,
        to: EntryId,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Message {
    Inconsistency(Inconsistency),
    Warning(Warning),
    Suggestion(Suggestion),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MessageType {
    Inconsistency,
    Warning,
    Suggestion,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Output(pub Vec<Message>);

pub trait InnerMessageType: Clone + Debug + PartialEq + Eq + Hash {
    fn ty(&self) -> MessageType;
}
pub trait InnerOutput<MSG: InnerMessageType>: Clone {
    fn push(&mut self, v: MSG);
}

//A Typed findings remain private to an analyzer until its explicit
//A normalization boundary. Views provide all graph access; setup cannot
//A observe the database or a future suffix.
pub trait Analyzer: Send + Sync {
    type MSG: InnerMessageType
    where
        Self: Sized;
    type OUTPUT: InnerOutput<Self::MSG>
    where
        Self: Sized;
    fn initialize_inner(&mut self) -> Self::OUTPUT
    where
        Self: Sized;
    fn analyze_inner(&self, view: View<'_>) -> Self::OUTPUT
    where
        Self: Sized;
    fn convert_report(&self, report: Self::OUTPUT) -> Output
    where
        Self: Sized;
    fn initialize(&mut self) -> Output;
    fn analyze(&self, view: View<'_>) -> Output;
}

pub trait AnalyzerFactory {
    fn analyzer_with_config(&mut self, config: &str) -> Result<Box<dyn Analyzer>, LifeCoreError>;
    fn analyzer_name(&self) -> &'static str;
}

#[derive(Default)]
pub struct AnalysisRunner {
    factory: BTreeMap<String, Box<dyn AnalyzerFactory>>,
}

impl AnalysisRunner {
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
    pub fn run(&mut self, graph: &LifeCoreList) -> Result<Output, LifeCoreError> {
        self.run_view(graph.view(), graph.analysis_suite())
    }
    pub fn run_view(
        &mut self,
        view: View<'_>,
        suite: &SuiteConfig,
    ) -> Result<Output, LifeCoreError> {
        let mut analyzers = vec![];
        for config in &suite.0 {
            let factory = self.factory.get_mut(&config.name).ok_or_else(|| {
                LifeCoreError::AnalyzerNotFound {
                    name: config.name.clone(),
                }
            })?;
            analyzers.push(factory.analyzer_with_config(&config.config)?);
        }
        let mut output = Output::default();
        for analyzer in &mut analyzers {
            output.0.extend(analyzer.initialize().0);
        }
        //A Preserve analyzer/configuration order and include the empty prefix even in
        //A a nonempty history.
        for analyzer in &analyzers {
            for end in 0..=view.len() {
                output.0.extend(
                    analyzer
                        .analyze(view.prefix(end).expect("bounded replay prefix"))
                        .0,
                );
            }
        }
        Ok(output)
    }
}

impl Output {
    pub fn has_error(&self) -> bool {
        self.errors().next().is_some()
    }
    pub fn errors(&self) -> impl Iterator<Item = &Inconsistency> {
        self.0.iter().filter_map(|m| match m {
        Message::Inconsistency(e) => Some(e),
        _ => None,
        })
    }
    pub fn warnings(&self) -> impl Iterator<Item = &Warning> {
        self.0.iter().filter_map(|m| match m {
        Message::Warning(w) => Some(w),
        _ => None,
        })
    }
    pub fn suggestions(&self) -> impl Iterator<Item = &Suggestion> {
        self.0.iter().filter_map(|m| match m {
        Message::Suggestion(s) => Some(s),
        _ => None,
        })
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

impl From<Inconsistency> for Message {
    fn from(v: Inconsistency) -> Self {
        Self::Inconsistency(v)
    }
}
impl From<Warning> for Message {
    fn from(v: Warning) -> Self {
        Self::Warning(v)
    }
}
impl From<Suggestion> for Message {
    fn from(v: Suggestion) -> Self {
        Self::Suggestion(v)
    }
}

// Special-Case Hash: relevance list should be sorted first
impl Hash for Warning {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.offending_event.hash(state);
        self.system_name.hash(state);
        self.system_config.hash(state);
        {
            let mut sorted = self.relevance.clone();
            sorted.sort();
            sorted.hash(state);
        }
        self.message.hash(state);
    }
}

// Special-Case Hash: relevance list should be sorted first
impl Hash for Suggestion {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.offending_event.hash(state);
        self.system_name.hash(state);
        self.system_config.hash(state);
        {
            let mut sorted = self.relevance.clone();
            sorted.sort();
            sorted.hash(state);
        }
        self.message.hash(state);

        // Actions already have an ordering to them;
        // sorting a suggested action list may turn it into an invalid sugestion
        self.action.hash(state);
    }
}

#[cfg(test)]
mod test;
