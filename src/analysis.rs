use std::collections::BTreeMap;
use std::fmt::Debug as DebugTrait;
use std::hash::Hash;

use itertools::Itertools;
use serde::{Deserialize, Serialize};
use strum::{EnumDiscriminants, EnumIs, EnumTryAs};

use crate::{Body, Context, DiagnosticLocation, EntryIndex, LifeCore, LifeCoreError};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SuiteConfig(pub Vec<Config>);

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
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

#[derive(PartialEq, Eq, Clone)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub struct Warning {
    pub offending_event: DiagnosticLocation,
    pub system_name: String,
    pub system_config: String,
    pub relevance: Vec<EntryIndex>,
    pub message: String,
}

#[derive(PartialEq, Eq, Clone)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub struct Suggestion {
    pub offending_event: DiagnosticLocation,
    pub system_name: String,
    pub system_config: String,
    pub relevance: Vec<EntryIndex>,
    pub message: String,
    pub action: Vec<SuggestedAction>,
}

#[derive(PartialEq, Eq, Clone, Hash)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub enum SuggestedAction {
    AddNode {
        ty_name: String,
        payload: Body,
        parent: Option<EntryIndex>,
    },
    AddEdge {
        from: EntryIndex,
        ty_name: String,
        to: EntryIndex,
    },
}

/// A Message from an `Analyzer`
///
/// # "Relevance"
/// Some `Message` variants have a field named `relevance`. Errors are always
/// "relevant" of course but warnings and suggestions less, as they may have
/// been resolved later. The UI may choose to ignore a message if N members of
/// `relevance` are stale (for some sensible N) Additionally, UI may implement a
/// `barklist` where messages of a certain hash are dismissed. (This led to
/// needing a custom `Hash` implementation that specifically sorts this list)
#[derive(PartialEq, Eq, Clone, EnumIs, EnumTryAs, Hash, EnumDiscriminants)]
#[strum_discriminants(name(MessageType))]
#[strum_discriminants(vis(pub))]
#[cfg_attr(debug_assertions, derive(Debug))]
pub enum Message {
    Inconsistency(Inconsistency),
    Warning(Warning),
    Suggestion(Suggestion),
}

#[derive(PartialEq, Eq, Clone)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub struct Output(pub Vec<Message>);

pub trait InnerMessageType: Clone + DebugTrait + PartialEq + Eq + Hash + Sized {
    fn ty(&self) -> MessageType;
}

pub trait InnerOutput<MSG: InnerMessageType>: Clone + Sized {
    fn push(&mut self, v: MSG);
}

/// Type representing an Analyzer, a piece of code that goes over the database
/// and generates messages `Analyzer` is bound by LifeCore trait implementor and
/// `InnerOutput`. `InnerOutput` is actually not strictly necessary but it
/// guides the implementor (or an AI agent) to create inner message types
/// that communicate the issue, rather than blocks of text inside `Message`.
/// `Into<Message>` is not feasible and only the Analyzer may have enough
/// context to convert `InnerOutput` to the all-purpose `Message`; so `Analyzer`
/// also has to implement conversion methods.
///
/// The idea behind `initialize` is that if some pre-condition is not quite
/// correct, or if we are "done" in some sense, analyzer can disable itself and
/// potentially issue a warning.
pub trait Analyzer<LC: LifeCore>: Send + Sync {
    type MSG: InnerMessageType
    where
        Self: Sized;
    type OUTPUT: InnerOutput<Self::MSG>
    where
        Self: Sized;
    fn initialize_inner(&mut self, lc: &LC) -> Self::OUTPUT
    where
        Self: Sized;
    fn analyze_inner(&self, lc: &LC, window: Context) -> Self::OUTPUT
    where
        Self: Sized;
    fn convert_report(&self, r: Self::OUTPUT) -> Output
    where
        Self: Sized;
    fn initialize(&mut self, lc: &LC) -> Output;
    fn analyze(&self, lc: &LC, window: Context) -> Output;
}

pub trait AnalyzerFactory<LC: LifeCore> {
    fn analyzer_with_config(
        &mut self,
        config: &str,
    ) -> Result<Box<dyn Analyzer<LC>>, LifeCoreError>;
    fn analyzer_name(&self) -> &'static str;
}

#[derive(Default)]
pub struct AnalysisRunner<LC: LifeCore> {
    factory: BTreeMap<String, Box<dyn AnalyzerFactory<LC>>>,
    analyzer: Vec<Box<dyn Analyzer<LC>>>,
}

impl<LC: LifeCore> AnalysisRunner<LC> {
    pub fn register_analyzer_kind(
        &mut self,
        factory: Box<dyn AnalyzerFactory<LC>>,
    ) -> Result<(), LifeCoreError> {
        if self.factory.contains_key(factory.analyzer_name()) {
            return Err(LifeCoreError::DuplicateAnalyzer {
                dupe_name: factory.analyzer_name().to_string(),
            });
        }
        self.factory
            .insert(factory.analyzer_name().to_string(), factory);
        Ok(())
    }

    pub fn run(&mut self, lc: &LC) -> Result<Output, LifeCoreError> {
        let mut analyzer: Vec<Box<dyn Analyzer<LC>>> = vec![];
        for x in lc.current_analysis_suite().0.iter() {
            if self.factory.contains_key(&x.name) {
                analyzer.push(
                    self.factory
                        .get_mut(&x.name)
                        .unwrap()
                        .analyzer_with_config(&x.config)?,
                );
            } else {
                return Err(LifeCoreError::AnalyzerNotFound {
                    name: x.name.clone(),
                });
            }
        }
        let mut r: Vec<Message> =
            analyzer
                .iter_mut()
                .map(|x| x.initialize(lc).0)
                .fold(vec![], |acc, x| {
                    let mut t = vec![];
                    t.extend(acc);
                    t.extend(x);
                    t
                });
        r.extend(
            analyzer
                .iter()
                .cartesian_product(
                    &lc.window(Context::universe().items())
                        .map(|x| x.entry_context().shrink_to_inclusive_left())
                        .collect_vec(), // TODO PERF get rid of this
                )
                .map(|(a, window)| a.analyze(&lc, *window).0)
                .fold(vec![], |mut acc, x| {
                    acc.extend(x);
                    acc
                }),
        );
        Ok(Output(r))
    }
}

impl Output {
    fn validate(&self) -> bool {
        // TODO what could we be validating here, just in case? And `assert!` or
        // `debug_assert!`? self.0.iter().all(|x| match x {
        //     Message::Inconsistency(inconsistency) => todo!(),
        //     Message::Warning(warning) => todo!(),
        //     Message::Suggestion(suggestion) => todo!(),
        // })
        true
    }

    pub fn has_error(&self) -> bool {
        debug_assert!(self.validate());
        self.0.iter().any(|x| x.is_inconsistency())
    }

    pub fn has_warning(&self) -> bool {
        debug_assert!(self.validate());
        self.0.iter().any(|x| x.is_warning())
    }

    pub fn has_suggestion(&self) -> bool {
        debug_assert!(self.validate());
        self.0.iter().any(|x| x.is_suggestion())
    }

    pub fn errors(&self) -> Vec<Inconsistency> {
        debug_assert!(self.validate());
        self.0
            .iter()
            .cloned()
            .flat_map(|x| x.try_as_inconsistency())
            .collect_vec()
    }

    pub fn warnings(&self) -> Vec<Warning> {
        debug_assert!(self.validate());
        self.0
            .iter()
            .cloned()
            .flat_map(|x| x.try_as_warning())
            .collect_vec()
    }

    pub fn suggestions(&self) -> Vec<Suggestion> {
        debug_assert!(self.validate());
        self.0
            .iter()
            .cloned()
            .flat_map(|x| x.try_as_suggestion())
            .collect_vec()
    }
}

impl From<Suggestion> for Message {
    fn from(value: Suggestion) -> Self {
        Self::Suggestion(value)
    }
}

impl From<Warning> for Message {
    fn from(value: Warning) -> Self {
        Self::Warning(value)
    }
}

impl From<Inconsistency> for Message {
    fn from(value: Inconsistency) -> Self {
        Self::Inconsistency(value)
    }
}

impl Into<LifeCoreError> for Output {
    fn into(self) -> LifeCoreError {
        assert!(self.has_error());
        LifeCoreError::SystemsError(
            self.0
                .into_iter()
                .flat_map(|x| x.try_as_inconsistency())
                .collect_vec(),
        )
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
