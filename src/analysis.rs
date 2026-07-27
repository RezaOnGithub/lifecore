use std::collections::BTreeMap;
use std::fmt::Debug as DebugTrait;
use std::hash::Hash;

use itertools::Itertools;
use serde::{Deserialize, Serialize};
use strum::{EnumDiscriminants, EnumIs, EnumTryAs};

use crate::{
    Body, Context, DiagnosticLocation, Edge, Entry, EntryContext, EntryIndex, LifeCore,
    LifeCoreAccess, LifeCoreError, LifeCoreFamily, Node, NodeQuery,
};

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

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct Warning {
    pub offending_event: DiagnosticLocation,
    pub system_name: String,
    pub system_config: String,
    pub relevance: Vec<EntryIndex>,
    pub message: String,
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct Suggestion {
    pub offending_event: DiagnosticLocation,
    pub system_name: String,
    pub system_config: String,
    pub relevance: Vec<EntryIndex>,
    pub message: String,
    pub action: Vec<SuggestedAction>,
}

#[derive(PartialEq, Eq, Clone, Hash, Debug)]
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

//A The harness exposes only read access.
//A Every context is capped at the runner's historical prefix.
pub struct LifeCoreAnalysisHarness<'a, LC: LifeCoreAccess> {
    lc: &'a LC,
    bounds: Context,
}

impl<'a, LC: LifeCoreAccess> LifeCoreAnalysisHarness<'a, LC> {
    pub fn new(lc: &'a LC, bounds: Context) -> Self {
        Self { lc, bounds }
    }

    fn valid_index(&self, e: EntryIndex) -> bool {
        self.bounds.end > e.0
    }

    fn constrain_context(&self, c: Context) -> Context {
        if self.bounds.end >= c.end {
            c
        } else {
            self.bounds
        }
    }

    fn constrain_entry_context(&self, e: EntryContext) -> Option<EntryContext> {
        if self.valid_index(e.index()) {
            Some(self.constrain_context(e.context()).look_at(e))
        } else {
            None
        }
    }
}

impl<LC: LifeCoreAccess> LifeCoreAccess for LifeCoreAnalysisHarness<'_, LC> {
    fn get(&self, e: EntryContext) -> Option<Entry> {
        self.lc.get(self.constrain_entry_context(e)?)
    }

    fn window<I>(&self, i: I) -> impl Iterator<Item = Entry>
    where
        I: IntoIterator<Item = EntryContext>,
    {
        i.into_iter().map_while(|x| self.get(x))
    }

    fn is_stale(&self, e: EntryContext) -> Option<bool> {
        self.lc.is_stale(self.constrain_entry_context(e)?)
    }

    fn query(&self, query: NodeQuery) -> Vec<Node> {
        self.lc
            .query(NodeQuery {
                context: self.constrain_context(query.context),
                expression: query.expression,
            })
            .into_iter()
            .filter_map(|mut node| {
                node.e = self.constrain_entry_context(node.e)?;
                Some(node)
            })
            .collect()
    }

    fn nodes(&self, ctx: Context) -> impl Iterator<Item = Node> {
        self.lc
            .nodes(self.constrain_context(ctx))
            .filter_map(|mut node| {
                node.e = self.constrain_entry_context(node.e)?;
                Some(node)
            })
    }

    fn edges(&self, ctx: Context) -> impl Iterator<Item = Edge> {
        self.lc
            .edges(self.constrain_context(ctx))
            .filter_map(|mut edge| {
                edge.e = self.constrain_entry_context(edge.e)?;
                Some(edge)
            })
    }

    fn root_of(&self, e: EntryContext) -> Option<EntryContext> {
        let root = self.lc.root_of(self.constrain_entry_context(e)?)?;
        self.constrain_entry_context(root)
    }

    fn fresh_successors(&self, node: EntryContext) -> Vec<EntryContext> {
        let Some(node) = self.constrain_entry_context(node) else {
            return vec![];
        };
        self.lc
            .fresh_successors(node)
            .into_iter()
            .filter_map(|successor| self.constrain_entry_context(successor))
            .collect()
    }

    fn relative_degree(&self, a: EntryContext, b: EntryContext) -> Option<usize> {
        self.lc.relative_degree(
            self.constrain_entry_context(a)?,
            self.constrain_entry_context(b)?,
        )
    }

    fn families(&self, ctx: Context) -> Vec<LifeCoreFamily> {
        self.lc.families(self.constrain_context(ctx))
    }
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
//A Initialization intentionally has no database access; all inspection happens
//A through a prefix-bounded harness during `analyze`.
pub trait Analyzer<LC: LifeCoreAccess>: Send + Sync {
    type MSG: InnerMessageType
    where
        Self: Sized;
    type OUTPUT: InnerOutput<Self::MSG>
    where
        Self: Sized;
    fn initialize_inner(&mut self) -> Self::OUTPUT
    where
        Self: Sized;
    fn analyze_inner(&self, lc: LifeCoreAnalysisHarness<'_, LC>, window: Context) -> Self::OUTPUT
    where
        Self: Sized;
    fn convert_report(&self, r: Self::OUTPUT) -> Output
    where
        Self: Sized;
    fn initialize(&mut self) -> Output;
    fn analyze(&self, lc: LifeCoreAnalysisHarness<'_, LC>, window: Context) -> Output;
}

pub trait AnalyzerFactory<LC: LifeCoreAccess> {
    fn analyzer_with_config(
        &mut self,
        config: &str,
    ) -> Result<Box<dyn Analyzer<LC>>, LifeCoreError>;
    fn analyzer_name(&self) -> &'static str;
}

#[derive(Default)]
pub struct AnalysisRunner<LC: LifeCore> {
    factory: BTreeMap<String, Box<dyn AnalyzerFactory<LC>>>,
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
                .map(|x| x.initialize().0)
                .fold(vec![], |acc, x| {
                    let mut t = vec![];
                    t.extend(acc);
                    t.extend(x);
                    t
                });
        let mut windows = lc
            .window(Context::universe().items())
            .map(|x| x.entry_context().shrink_to_inclusive_left())
            .collect_vec();
        if windows.is_empty() {
            //A An empty database still has one bounded historical prefix.
            windows.push(EntryIndex(0).before());
        }
        r.extend(
            analyzer
                .iter()
                .cartesian_product(&windows)
                .map(|(a, window)| {
                    a.analyze(LifeCoreAnalysisHarness::new(lc, *window), *window)
                        .0
                })
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
        // `debug_assert!`?

        // self.0.iter().all(|x| match x {
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

#[cfg(test)]
mod test;
