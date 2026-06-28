use crate::analyzer::Analyzer;


pub struct TypeCheck;

impl Analyzer for TypeCheck {
    fn initialize(lc: &mut crate::LifeCore, config: &str) -> Result<Self, crate::LifeCoreError> {
        todo!()
    }

    fn analyze(self, lc: &crate::LifeCore) -> Vec<crate::AnalysisReport> {
        todo!()
    }
}