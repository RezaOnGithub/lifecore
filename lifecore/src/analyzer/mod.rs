use serde::{Deserialize, Serialize};
use strum::EnumDiscriminants;

pub(crate) mod dagcheck;
pub(crate) mod typecheck;

use crate::analyzer::dagcheck::DAGCheck;
use crate::analyzer::typecheck::TypeCheck;
use crate::{AnalysisReport, LifeCore, LifeCoreError};

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnalysisSuite(pub Vec<(AnalyzerKind, String)>);

// Boilerplate. Boxing analyzers proved to be tricky, and I'm the only user for now.
// So in the end, this is just for code organization, and maybe a sketch of a potential public interface.

#[derive(EnumDiscriminants)]
#[strum_discriminants(derive(serde::Serialize, serde::Deserialize))]
#[strum_discriminants(vis(pub))]
#[strum_discriminants(name(AnalyzerKind))]
enum AnalysisSuiteInstance {
    TypeCheck { checker: TypeCheck, config: String },
    DAGCheck { checker: DAGCheck, config: String },
}

pub trait Analyzer: Sized {
    fn initialize(lc: &mut LifeCore, config: &str) -> Result<Self, LifeCoreError>;
    fn analyze(self, lc: &LifeCore) -> Vec<AnalysisReport>;
}

// Copied from static_assertions 1.1.0
macro_rules! assert_impl_all {
    ($type:ty: $($trait:path),+ $(,)?) => {
        const _: fn() = || {
            // Only callable when `$type` implements all traits in `$($trait)+`.
            fn assert_impl_all<T: ?Sized $(+ $trait)+>() {}
            assert_impl_all::<$type>();
        };
    };
}

impl AnalysisSuiteInstance {
    pub fn new(lc: &mut LifeCore, ty: AnalyzerKind, config: String) -> Result<Self, LifeCoreError> {
        match ty {
            // COPY_PASTE
            AnalyzerKind::TypeCheck => {
                assert_impl_all!(TypeCheck: Analyzer);
                let checker = TypeCheck::initialize(lc, config.as_str())?;
                Ok(Self::TypeCheck { checker, config })
            }
            AnalyzerKind::DAGCheck => {
                assert_impl_all!(DAGCheck: Analyzer);
                let checker = DAGCheck::initialize(lc, config.as_str())?;
                Ok(Self::DAGCheck { checker, config })
            }
        }
    }
}

impl Analyzer for AnalysisSuiteInstance {
    fn initialize(_lc: &mut LifeCore, _config: &str) -> Result<Self, LifeCoreError> {
        panic!("Nope!");
    }

    fn analyze(self, lc: &LifeCore) -> Vec<AnalysisReport> {
        match self {
            AnalysisSuiteInstance::TypeCheck { checker, config: _ } => checker.analyze(lc),
            AnalysisSuiteInstance::DAGCheck { checker, config: _ } => checker.analyze(lc),
        }
    }
}
