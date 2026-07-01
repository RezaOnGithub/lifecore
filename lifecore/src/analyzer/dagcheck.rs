use crate::analyzer::Analyzer;

pub struct DAGCheck;

impl Analyzer for DAGCheck {
    fn initialize(lc: &mut crate::LifeCore, config: &str) -> Result<Self, crate::LifeCoreError> {
        todo!()
    }

    fn analyze(self, lc: &crate::LifeCore) -> Vec<crate::AnalysisReport> {
        todo!()
    }
}