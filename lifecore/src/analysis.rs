//! This particular design is just an excuse to learn about "dyn". It is absolutely not an ideal design
//! (But it could serve as a sketch for a future plugin system)

use std::collections::HashMap;

use crate::{AnalysisOutput, AnalysisReport, LifeCore, LifeCoreError};
use rayon::iter::ParallelIterator;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnalysisSuiteConfig(pub Vec<AnalysisConfig>);

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnalysisConfig {
    pub name: String,
    pub config: String,
}

pub trait Analyzer {
    fn initialize(&mut self, lc: &LifeCore) -> AnalysisOutput;
    fn analyze(&mut self, lc: &LifeCore) -> AnalysisOutput;
}

pub trait AnalyzerFactory {
    fn analyzer_with_config(&mut self, config: &str) -> Result<Box<dyn Analyzer>, LifeCoreError>;
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

    pub fn register_analyzer_kind(
        &mut self,
        name: &str,
        factory: Box<dyn AnalyzerFactory>,
    ) -> Result<(), LifeCoreError> {
        if self.factory.contains_key(name) {
            return Err(LifeCoreError::DuplicateAnalyzer {
                dupe_name: name.to_string(),
            });
        }
        self.factory.insert(name.to_string(), factory);
        Ok(())
    }

    pub fn run(&mut self, lc: &LifeCore) -> Result<Vec<AnalysisReport>, LifeCoreError> {
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
        let mut r: Vec<AnalysisReport> = vec![];
        Ok(r)
    }
}
