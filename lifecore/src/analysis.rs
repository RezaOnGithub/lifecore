//! This particular design is just an excuse to learn about "dyn". It is absolutely not an ideal design
//! (But it could serve as a sketch for a future plugin system)

use crate::{AnalysisOutput, AnalysisReport, LifeCore, LifeCoreError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnalysisSuiteConfig(pub Vec<AnalysisConfig>);

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnalysisConfig {
    pub name: String,
    pub config: String,
}

// The idea behind `initialize` is that if some pre-condition is not quite correct, or
// if we are "done" in some sense, analyzer can disable itself and maybe issue a warning.
pub trait Analyzer : Send + Sync {
    fn initialize(&mut self, lc: &LifeCore) -> AnalysisOutput;
    fn analyze(&self, lc: &LifeCore) -> AnalysisOutput;
}

pub trait AnalyzerFactory {
    fn analyzer_with_config(&mut self, config: &str) -> Result<Box<dyn Analyzer>, LifeCoreError>;
    fn analyzer_name(&self) -> &'static str;
}

pub struct AnalysisRunner {
    factory: HashMap<String, Box<dyn AnalyzerFactory>>,
    analyzer: Vec<Box<dyn Analyzer>>,
}

impl AnalysisRunner {
    pub fn new() -> Self {
        Self {
            factory: HashMap::new(),
            analyzer: vec![],
        }
    }

    #[must_use]
    pub fn register_analyzer_kind(
        &mut self,
        factory: Box<dyn AnalyzerFactory>,
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

    pub fn run(&mut self, lc: &LifeCore) -> Result<AnalysisOutput, LifeCoreError> {
        let mut analyzer: Vec<Box<dyn Analyzer>> = vec![];
        for x in lc.configured_analyzer.0.iter() {
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
        let mut r: Vec<AnalysisReport> =
            analyzer
                .iter_mut()
                .map(|x| x.initialize(lc).0)
                .fold(vec![], |acc, x| {
                    let mut t = vec![];
                    t.extend(acc.into_iter());
                    t.extend(x.into_iter());
                    t
                });
        r.extend(
            analyzer
                .into_iter()
                .map(|x| x.analyze(lc).0)
                .fold(vec![], |acc, x| {
                    let mut t = vec![];
                    t.extend(acc.into_iter());
                    t.extend(x.into_iter());
                    t
                }),
        );
        Ok(AnalysisOutput(r))
    }
}
