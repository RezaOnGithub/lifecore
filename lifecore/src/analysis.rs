//! This particular design is just an excuse to learn about "dyn". It is absolutely not an ideal design
//! (But it could serve as a sketch for a future plugin system)

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use strum::EnumDiscriminants;

pub(crate) mod dagcheck;
pub(crate) mod typecheck;

use crate::analyzer::dagcheck::DAGCheck;
use crate::analyzer::typecheck::TypeCheck;
use crate::{AnalysisOutput, AnalysisReport, LifeCore, LifeCoreError};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnalysisSuiteConfig(pub Vec<AnalysisConfig>);

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnalysisConfig{
    pub name: String,
    pub config: String,
}

pub trait Analyzer {
    fn initialize(&mut self, lc: &LifeCore) -> AnalysisOutput;
    fn analyze(&mut self, lc: &LifeCore) -> AnalysisOutput;
}

pub trait AnalyzerFactory {
    fn analyzer_with_config(&mut self, config: &str) -> Box<dyn Analyzer>;
}

pub struct AnalysisRunner {
    factory: HashMap<String,Box<dyn AnalyzerFactory>>,
    analyzer: Vec<Box<dyn Analyzer>>
}

impl AnalysisRunner {
    pub fn new() -> Self {
        Self {
            factory: HashMap::new(),
            analyzer: vec![]
        }
    }

    pub fn register_analyzer_kind(&mut self, name: &str, factory: Box<dyn AnalyzerFactory>) -> Result<(), LifeCoreError> {
        // self.factory.contains_key(name) {
        //     Err(LifeCoreError::)
        // }
    }
}