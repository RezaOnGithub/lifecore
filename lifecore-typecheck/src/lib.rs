//! "Type System" Constraint System
//!
//! This implements a basic type checking system that is compatible with the limited VCS capabilities.
//! A "Node_Type" declares a new type with a unique name among non-stale nodes, the edges that are
//! allowed to connect to or from the node, and a node class: Toothpick (only a single edge that is
//! outgoing), Hub (only incoming edges), Spike (only outgoing edges), Regular (no restrictions) EdgeNode
//! (exactly one incoming and exactly one outgoing)
//!
//! The general idea is: at every event, all node must get typechecked against exactly one non-stale Node_Type
//! that is older or the same age; That is, if a node type X is overriden at E, all non-stale X nodes by E must
//! typecheck against the new type X' that overrode X.
//!
//! This can be terribly inefficient, and of course there is a streaming approach but correctness for now. NOTDO PERF

use itertools::Itertools;
use lifecore::{
    AnalysisOutput, AnalysisReport, Analyzer, AnalyzerFactory, ConstraintSystemMessage, EventContext, LifeCore, LifeCoreError, NodeType, NodeTypeFieldStatus, PayloadType, SuggestedAction, TypedBody,
};
use std::{collections::HashMap, marker::PhantomData};
use strum::EnumProperty;

#[cfg(test)]
mod test;

mod typecheck;

const TYPECHECK_NAME: &str = "Type_Check";

pub struct TypeCheckFactory<LC: LifeCore> {
    _x: PhantomData<LC>,
}

impl<LC: LifeCore> Default for TypeCheckFactory<LC> {
    fn default() -> Self {
        Self {
            _x: Default::default(),
        }
    }
}

impl<LC: LifeCore> AnalyzerFactory<LC> for TypeCheckFactory<LC> {
    fn analyzer_with_config(
        &mut self,
        config: &str,
    ) -> Result<Box<dyn Analyzer<LC>>, LifeCoreError> {
        Ok(Box::new(TypeCheck {
            config: config.to_string(),
        }))
    }

    fn analyzer_name(&self) -> &'static str {
        TYPECHECK_NAME
    }
}

pub struct TypeCheck {
    pub config: String,
}

#[derive(EnumProperty)]
#[allow(dead_code)] // due to strum::EnumProperty
pub enum TypeCheckReport {
    #[strum(props(kind = "Suggestion"))]
    AddBootstrapNode,
    #[strum(props(kind = "Error"))]
    MalformedNodeTypeBody { offending_event: EventContext },
    #[strum(props(kind = "Error"))]
    DuplicateTypeDefinition {
        offending_event: EventContext,
        name: String,
    },
    #[strum(props(kind = "Warning"))]
    NonexistantOverrideType {
        offending_event: EventContext,
        base_ty_name: String,
        override_ty_name: String,
        relevance: Vec<EventContext>,
    },
    #[strum(props(kind = "Error"))]
    NodeTypeNonexistant {
        offending_event: EventContext,
        ty_name: String,
    },
    #[strum(props(kind = "Error"))]
    ElidedMandatoryField {
        offending_event: EventContext,
        ty_name: String,
        field_name: String,
    },
    #[strum(props(kind = "Warning"))]
    ElidedEncouragedField {
        offending_event: EventContext,
        ty_name: String,
        field_name: String,
    },
    #[strum(props(kind = "Warning"))]
    DeprecatedField {
        offending_event: EventContext,
        ty_name: String,
        field_name: String,
    },
    #[strum(props(kind = "Error"))]
    FieldTypeMismatch {
        offending_event: EventContext,
        ty_name: String,
        field_name: String,
        expected_type: PayloadType,
        present_type: PayloadType,
    },
    #[strum(props(kind = "Error"))]
    ExtraneousField {
        offending_event: EventContext,
        ty_name: String,
        field_name: String,
    },
    #[strum(props(kind = "Error"))]
    BadEdgeEndpoint {
        offending_event: EventContext,
        edge_ty_name: String,
        bad_node_ty_name: String,
        bad_node_is_head: bool
    }
}

impl TypeCheckReport {
    fn is_x(&self, x: &str) -> bool {
        self.get_str("kind").is_some_and(|y| y == x)
    }

    pub fn is_suggestion(&self) -> bool {
        self.is_x("Suggestion")
    }

    pub fn is_warning(&self) -> bool {
        self.is_x("Warning")
    }

    pub fn is_error(&self) -> bool {
        self.is_x("Error")
    }

    pub fn suggested_action(&self) -> SuggestedAction {
        if !self.is_suggestion() {
            panic!("Not a suggested action")
        }
        match self {
        TypeCheckReport::AddBootstrapNode => SuggestedAction::AddNode {
            ty_name: NodeType::name().to_string(),
            payload: NodeType::describe_self().into(),
            parent: None,
        },
        _ => unimplemented!("missing suggested action for a suggestion"),
        }
    }

    pub fn offending_event(&self) -> EventContext {
        todo!()
    }

    pub fn message(&self) -> String {
        todo!()
    }

    pub fn relevance_list(&self) -> Vec<EventContext> {
        todo!()
    }
}

impl TypeCheck {
    fn convert_report(&self, r: TypeCheckReport) -> AnalysisReport {
        match r {
        TypeCheckReport::AddBootstrapNode => AnalysisReport::Suggestion(
            ConstraintSystemMessage {
                offending_event: EventContext::universe(),
                system_name: TYPECHECK_NAME.to_string(),
                system_config: self.config.clone(),
                relevance: vec![],
                message: format!("Bootstrap the built-in {} declaration", NodeType::name()),
            },
            SuggestedAction::AddNode {
                ty_name: NodeType::name().to_string(),
                payload: NodeType::describe_self().into(),
                parent: None,
            },
        ),
        }
    }
}

pub struct TypeCheckOutput(pub Vec<TypeCheckReport>);

impl TypeCheckOutput {
    pub fn validate(&self) -> bool {
        fn xor_all(values: impl IntoIterator<Item = bool>) -> bool {
            values.into_iter().fold(false, |acc, value| acc ^ value)
        }
        self.0
            .iter()
            .all(|x| xor_all([x.is_error(), x.is_suggestion(), x.is_warning()]))
    }

    pub fn push(&mut self, v: TypeCheckReport) {
        self.0.push(v)
    }
}

impl<LC: LifeCore> Analyzer<LC> for TypeCheck {
    // TODO Node_Type node type has `Tombstone` as an additional override. suggest it.
    fn initialize(&mut self, lc: &LC) -> AnalysisOutput {
        let r = self.typecheck_initialize(lc);
        assert!(r.validate());
        lifecore::AnalysisOutput(
            r.0.into_iter()
                .map(|x| self.convert_report(x))
                .collect_vec(),
        )
    }

    fn analyze(&self, lc: &LC) -> AnalysisOutput {
        let r = self.typecheck_analyze(lc);
        assert!(r.validate());
        lifecore::AnalysisOutput(
            r.0.into_iter()
                .map(|x| self.convert_report(x))
                .collect_vec(),
        )
    }
}
