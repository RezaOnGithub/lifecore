//! The planning analyzer of [PLANNING](../../docs/PLANNING.md): reviews as
//! the clock, stances as commitment, and pressure as a valuation.
//!
//! The analyzer judges structural rules where an entry of planning's
//! vocabulary appears (§8.1 to §8.3) and the protocol at each seal (§8.4 and
//! §8.5). The valuation (§9) is the separate function [`valuation`], not an
//! analyzer output.

mod config;
mod current;
mod declarations;
mod edges;
mod findings;
mod nodes;
mod payload;
mod reminders;
mod seal;
mod situation;
mod valuation;
mod vocabulary;
mod windows;

pub use config::{PlanningConfig, PlanningConfigError};
pub use current::ItemState;
pub use findings::{Completion, FieldRule, PlanningMessage, Requirement, Superseded};
pub use payload::{PayloadError, Problem};
pub use valuation::{Ranked, Reason, RenderedReason, Valuation, ValuationOptions, valuation};
pub use vocabulary::{Label, PlanningType, StanceKind};

use lifecore::{
    Entry, LifeCoreError, View,
    analysis::{
        Analyzer, AnalyzerCertificate, AnalyzerFactory, Diagnostic, EvaluationInputs,
        InnerMessageType, Message, MessageKind, MessageType, TypedAnalyzer,
    },
    transaction::{EntryChange, NewNode},
    typing::Type,
};
use semver::Version;

pub struct Planning {
    config: PlanningConfig,
}

impl Planning {
    pub fn new(config: PlanningConfig) -> Self {
        Self { config }
    }

    // Activation (§8): an entry of planning's vocabulary, which is a node of
    // one of its ten types, a node that supersedes one, or an edge with one
    // of its four labels; a declaration that claims one of the ten names;
    // and a review, which is also a seal. Every other prefix is judged by
    // nothing here.
    fn judge_newest(&self, view: View<'_>) -> Result<Vec<PlanningMessage>, PayloadError> {
        let mut findings = Vec::new();
        match view.entries().next_back() {
        None => {}
        Some(Entry::Node(node)) => {
            if node.ty_name == Type::TYPE_NAME {
                findings.extend(declarations::judge(node));
            }
            let supersedes_planning_node = node
                .predecessor
                .is_some_and(|predecessor| current::planning_type(view, predecessor).is_some());
            if PlanningType::from_name(&node.ty_name).is_some() || supersedes_planning_node {
                findings.extend(nodes::judge(view, node, &self.config)?);
            }
            if node.ty_name == PlanningType::Review.name() {
                findings.extend(seal::judge(view, node, &self.config)?);
            }
        }
        Some(Entry::Edge(edge)) => {
            if let Some(label) = Label::from_name(&edge.ty_name) {
                findings.extend(edges::judge(view, edge, label)?);
            }
        }
        }
        Ok(findings)
    }

    // The stale-review nudge is the analyzer's only use of evaluation time,
    // and only when configured (decision 32).
    fn evaluation_time(
        &self,
        inputs: &EvaluationInputs,
    ) -> Result<Option<lifecore::Timestamp>, String> {
        match (self.config.stale_review_after_days, inputs.evaluation_time) {
        (None, _) => Ok(None),
        (Some(_), Some(now)) => Ok(Some(now)),
        (Some(_), None) => Err(
            "`stale_review_after_days` is configured, so the planning analyzer requires an evaluation time"
                .into(),
        ),
        }
    }
}

impl TypedAnalyzer for Planning {
    type MSG = PlanningMessage;

    fn version(&self) -> Version {
        Version::parse(env!("CARGO_PKG_VERSION")).expect("valid package version")
    }

    fn initialize_inner(&mut self, inputs: &EvaluationInputs) -> Result<Vec<Self::MSG>, String> {
        self.evaluation_time(inputs)?;
        Ok(Vec::new())
    }

    fn analyze_inner(
        &self,
        view: View<'_>,
        inputs: &EvaluationInputs,
    ) -> Result<Vec<Self::MSG>, String> {
        let now = self.evaluation_time(inputs)?;
        // The nonsticky messages hold at every prefix where their condition
        // does, judged or not.
        let mut findings = reminders::bootstrap(view);
        if let (Some(days), Some(now)) = (self.config.stale_review_after_days, now) {
            findings.extend(reminders::stale_review(view, days, now).map_err(|e| e.to_string())?);
        }
        findings.extend(self.judge_newest(view).map_err(|e| e.to_string())?);
        Ok(findings)
    }

    fn convert_message(&self, finding: Self::MSG) -> Message {
        let kind = match &finding {
        PlanningMessage::Bootstrap { name } => {
            MessageKind::Suggestion(vec![EntryChange::Node(NewNode {
                ty_name: Type::TYPE_NAME.into(),
                predecessor: None,
                intent: None,
                payload: name.canonical().to_body(),
            })])
        }
        _ => match finding.ty() {
        MessageType::Warning => MessageKind::Warning,
        _ => MessageKind::Inconsistency,
        },
        };
        Message {
            analyzer_certificate: AnalyzerCertificate(
                serde_json::to_string(&finding).expect("typed planning finding"),
            ),
            diagnostic: Diagnostic {
                subjects: finding.subjects(),
                focus: finding.focus(),
                message: finding.description(),
                kind,
                sticky: finding.sticky(),
            },
        }
    }
}

#[derive(Default)]
pub struct PlanningFactory;

impl PlanningFactory {
    pub fn name() -> &'static str {
        "Planning"
    }
}

impl AnalyzerFactory for PlanningFactory {
    fn analyzer_with_config(
        &mut self,
        configuration: &str,
    ) -> Result<Box<dyn Analyzer>, LifeCoreError> {
        let config = PlanningConfig::from_json(configuration).map_err(|error| {
            LifeCoreError::InvalidAnalyzerConfiguration {
                analyzer_name: Self::name().to_string(),
                message: error.to_string(),
            }
        })?;
        Ok(Box::new(Planning::new(config)))
    }

    fn analyzer_name(&self) -> &'static str {
        Self::name()
    }
}

#[cfg(test)]
mod test;
