//! "Type System" Constraint System
//!
//! This implements a basic type checking system that is compatible with the
//! limited VCS capabilities. A "Node_Type" declares a new type with a unique
//! name among non-stale types, and declares node fields, the edges that are
//! allowed to connect to or from the node, and the node class: Toothpick (only
//! a single edge that is outgoing), Hub (only incoming edges), Spike (only
//! outgoing edges), Regular (no restrictions) EdgeNode (exactly one incoming
//! and exactly one outgoing)
//!
//! The general idea is: at every event, all node must get typechecked against
//! exactly one non-stale Node_Type that is older or the same age; That is, if a
//! node type X is overriden at E, all non-stale X nodes by E must
//! typecheck against the new type X' that overrode X.
//!
//! This can be terribly inefficient, and of course there is a streaming
//! approach but correctness for now. NOTDO PERF

use itertools::Itertools;
use lifecore::{
    EventContext, LifeCore, LifeCoreError, NodeType, NodeTypeClass, NodeTypeFieldStatus,
    PayloadType, PointedEventContext, TypedBody,
    analysis::{self, InnerMessageType, InnerOutput},
};
use std::{collections::BTreeMap, marker::PhantomData};
use strum::IntoDiscriminant;

#[derive(Serialize, Deserialize, PartialEq, Eq, Hash, Clone)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub struct NodeType {
    pub name: String,
    pub class: NodeTypeClass,
    pub outgoing: Vec<String>,
    pub incoming: Vec<String>,
    pub additional_override: Vec<String>,
    pub field: Vec<(String, (ValueType, NodeTypeFieldStatus))>,
}

/// Describes the type and shape of a field's value
#[derive(Clone, PartialEq, Eq, Hash, strum::EnumIs, strum::EnumTryAs)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub enum ValueType {
    PreciseTime,
    Text,
    Number,

    Sequence(Box<ValueType>),
    Tuple(Vec<ValueType>),
    Set(Box<ValueType>),
    OneOf(Vec<ValueType>),

    Serialized,
}

/// Describes a Field as a whole
#[derive(Clone, PartialEq, Eq, Hash)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub struct FieldDecl {
    name: String,
    status: FieldStatus,
    ty: ValueType,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(test, derive(strum::EnumDiscriminants))]
#[cfg_attr(test, strum_discriminants(name(TypeCheckTest)))]
#[cfg_attr(test, strum_discriminants(vis(pub(crate))))]
#[cfg_attr(
    test,
    strum_discriminants(derive(strum::EnumIter, strum::VariantArray))
)]
pub enum TypeCheckMessage {
    AddBootstrapNode,
    MalformedNodeTypeBody {
        offending_event: PointedEventContext,
    },
    DuplicateTypeDefinition {
        offending_event: PointedEventContext,
        name: String,
    },
    NonexistantOverrideType {
        offending_event: PointedEventContext,
        base_ty_name: String,
        override_ty_name: String,
        relevance: Vec<PointedEventContext>,
    },
    NodeTypeNonexistant {
        offending_event: PointedEventContext,
        ty_name: String,
    },
    ElidedMandatoryField {
        offending_event: PointedEventContext,
        ty_name: String,
        field_name: String,
    },
    // TODO add relevance
    ElidedEncouragedField {
        offending_event: PointedEventContext,
        ty_name: String,
        field_name: String,
    },
    // TODO add relevance
    DeprecatedField {
        offending_event: PointedEventContext,
        ty_name: String,
        field_name: String,
    },
    FieldTypeMismatch {
        offending_event: PointedEventContext,
        ty_name: String,
        field_name: String,
        expected_type: PayloadType,
        present_type: PayloadType,
    },
    ExtraneousField {
        offending_event: PointedEventContext,
        ty_name: String,
        field_name: String,
    },
    BadEdgeEndpoint {
        offending_event: PointedEventContext,
        edge_ty_name: String,
        bad_node_ty_name: String,
        bad_node_is_head: bool,
    },
    BadOverride {
        offending_event: PointedEventContext,
        parent_ty_name: String,
        child_ty_name: String,
    },
    NodeClassNotRespected {
        offending_event: PointedEventContext,
        ty_name: String,
        expected_class: NodeTypeClass,
    },
}

#[derive(Clone)]
pub struct TypeCheckOutput(pub Vec<TypeCheckMessage>);

pub struct TypeCheckFactory<LC: LifeCore> {
    _x: PhantomData<LC>,
}

impl<LC: LifeCore> TypeCheckFactory<LC> {
    pub fn name() -> &'static str {
        "Type_Check"
    }
}

pub struct TypeCheck<LC: LifeCore> {
    pub config: String,
    _lc: PhantomData<LC>,
}

// impl Analyzer

impl InnerMessageType for TypeCheckMessage {
    fn ty(&self) -> analysis::MessageType {
        use analysis::MessageType::*;
        match self {
        TypeCheckMessage::AddBootstrapNode => Suggestion,
        TypeCheckMessage::MalformedNodeTypeBody { .. } => Inconsistency,
        TypeCheckMessage::DuplicateTypeDefinition { .. } => Inconsistency,
        TypeCheckMessage::NonexistantOverrideType { .. } => Warning,
        TypeCheckMessage::NodeTypeNonexistant { .. } => Inconsistency,
        TypeCheckMessage::ElidedMandatoryField { .. } => Inconsistency,
        TypeCheckMessage::ElidedEncouragedField { .. } => Warning,
        TypeCheckMessage::DeprecatedField { .. } => Warning,
        TypeCheckMessage::FieldTypeMismatch { .. } => Inconsistency,
        TypeCheckMessage::ExtraneousField { .. } => Inconsistency,
        TypeCheckMessage::BadEdgeEndpoint { .. } => Inconsistency,
        TypeCheckMessage::BadOverride { .. } => Inconsistency,
        TypeCheckMessage::NodeClassNotRespected { .. } => Inconsistency,
        }
    }
}

impl<LC: LifeCore> TypeCheck<LC> {
    fn convert_message(&self, x: TypeCheckMessage) -> analysis::Message {
        use analysis::{Inconsistency, SuggestedAction::*, Suggestion, Warning};
        let system_name = TypeCheckFactory::<LC>::name().to_string();
        let system_config = self.config.to_string();
        let r: analysis::Message = match x.clone() {
        TypeCheckMessage::AddBootstrapNode => Suggestion {
            offending_event: EventContext::universe().into(),
            system_name: TypeCheckFactory::<LC>::name().to_string(),
            system_config: self.config.to_string(),
            relevance: vec![],
            message: "Consider adding the bootstrap node for TypeCheck to work".to_string(),
            action: vec![AddNode {
                ty_name: NodeType::name().to_string(),
                payload: NodeType::describe_self().into(),
                parent: None,
            }],
        }
        .into(),
        TypeCheckMessage::MalformedNodeTypeBody { offending_event } => Inconsistency {
            offending_event: offending_event.into(),
            system_name,
            system_config,
            message: "Unable to parse NodeType body".to_string(),
        }
        .into(),
        TypeCheckMessage::DuplicateTypeDefinition {
            offending_event,
            name,
        } => Inconsistency {
            offending_event: offending_event.into(),
            system_name,
            system_config,
            message: format!(
                "Type {} is defined multiple times within the same window",
                name.as_str()
            ),
        }
        .into(),
        TypeCheckMessage::NonexistantOverrideType {
            offending_event,
            base_ty_name,
            override_ty_name,
            relevance,
        } => Warning {
            offending_event: offending_event.into(),
            system_name,
            system_config,
            relevance,
            message: format!(
                "Override type {} (referred to in {}) does not (yet) exist.",
                override_ty_name.as_str(),
                base_ty_name.as_str()
            ),
        }
        .into(),
        TypeCheckMessage::NodeTypeNonexistant {
            offending_event,
            ty_name,
        } => Inconsistency {
            offending_event: offending_event.into(),
            system_name,
            system_config,
            message: format!(
                "Type {} couldn't be resolved within the window.",
                ty_name.as_str()
            ),
        }
        .into(),
        TypeCheckMessage::ElidedMandatoryField {
            offending_event,
            ty_name,
            field_name,
        } => Inconsistency {
            offending_event: offending_event.into(),
            system_name,
            system_config,
            message: format!(
                "Mandatory field {} on a node of type {} is not present.",
                field_name.as_str(),
                ty_name.as_str()
            ),
        }
        .into(),
        TypeCheckMessage::ElidedEncouragedField {
            offending_event,
            ty_name,
            field_name,
        } => Warning {
            offending_event: offending_event.into(),
            system_name,
            system_config,
            relevance: vec![],
            message: format!(
                "Field {} on a node of type {} is empty; you are encouraged to fill it out.",
                field_name.as_str(),
                ty_name.as_str()
            ),
        }
        .into(),
        TypeCheckMessage::DeprecatedField {
            offending_event,
            ty_name,
            field_name,
        } => Warning {
            offending_event: offending_event.into(),
            system_name,
            system_config,
            relevance: vec![],
            message: format!(
                "Field {} on node type {} is deprecated; consider clearing it.",
                field_name.as_str(),
                ty_name.as_str()
            ),
        }
        .into(),
        TypeCheckMessage::FieldTypeMismatch {
            offending_event,
            ty_name,
            field_name,
            expected_type,
            present_type,
        } => Inconsistency {
            offending_event: offending_event.into(),
            system_name,
            system_config,
            message: format!(
                "Field {} on a node of type {} has the wrong type; expected {}, got {}.",
                field_name.as_str(),
                ty_name.as_str(),
                expected_type,
                present_type
            ),
        }
        .into(),
        TypeCheckMessage::ExtraneousField {
            offending_event,
            ty_name,
            field_name,
        } => Inconsistency {
            offending_event: offending_event.into(),
            system_name,
            system_config,
            message: format!(
                "Field {} on node type {} does not exist.",
                field_name.as_str(),
                ty_name.as_str()
            ),
        }
        .into(),
        TypeCheckMessage::BadEdgeEndpoint {
            offending_event,
            edge_ty_name,
            bad_node_ty_name,
            bad_node_is_head,
        } => Inconsistency {
            offending_event: offending_event.into(),
            system_name,
            system_config,
            message: format!(
                "Edge of type {} has incompatible {} type {}",
                edge_ty_name.as_str(),
                { if bad_node_is_head { " head" } else { "tail" } },
                bad_node_ty_name.as_str()
            ),
        }
        .into(),
        TypeCheckMessage::BadOverride {
            offending_event,
            parent_ty_name,
            child_ty_name,
        } => Inconsistency {
            offending_event: offending_event.into(),
            system_name,
            system_config,
            message: format!(
                "Parent of type {} does not allow a child of type {}.",
                parent_ty_name.as_str(),
                child_ty_name.as_str()
            ),
        }
        .into(),
        TypeCheckMessage::NodeClassNotRespected {
            offending_event,
            ty_name,
            expected_class,
        } => Inconsistency {
            offending_event: offending_event.into(),
            system_name,
            system_config,
            message: format!(
                "Node class {} of type {} is not respected.",
                expected_class,
                ty_name.as_str()
            ),
        }
        .into(),
        };
        assert!(r.discriminant() == x.ty());
        r
    }
}

impl analysis::InnerOutput<TypeCheckMessage> for TypeCheckOutput {
    fn push(&mut self, v: TypeCheckMessage) {
        self.0.push(v);
    }
}

impl<LC: LifeCore> analysis::Analyzer<LC> for TypeCheck<LC> {
    type MSG
        = TypeCheckMessage
    where
        Self: Sized;

    type OUTPUT
        = TypeCheckOutput
    where
        Self: Sized;
    fn initialize_inner(&mut self, lc: &LC) -> TypeCheckOutput
    where
        Self: Sized,
    {
        if lc
            .types(EventContext::universe())
            .any(|(name, _)| name == NodeType::name())
        {
            return TypeCheckOutput(vec![]);
        }

        TypeCheckOutput(vec![TypeCheckMessage::AddBootstrapNode])
    }

    fn analyze_inner(&self, lc: &LC, window: EventContext) -> TypeCheckOutput
    where
        Self: Sized,
    {
        use TypeCheckMessage::*;
        let mut r = TypeCheckOutput(vec![]);
        // check whether a Node_Type declaring some type X is not clashing with an older
        // but non-stale Node_Type declaring a type of the same name.
        for some_type_at_some_point_node in lc
            .nodes(window)
            .filter(|node| node.ty_name.as_str() == NodeType::name())
        {
            let type_table = lc
                .types(some_type_at_some_point_node.e.shrink_to_exclusive_left())
                .collect::<BTreeMap<_, _>>();

            let Ok(some_type_at_some_point) =
                NodeType::try_from(&some_type_at_some_point_node.field)
            else {
                r.push(MalformedNodeTypeBody {
                    offending_event: some_type_at_some_point_node.e,
                });
                continue;
            };
            if let Some((older_type_index, _older_type)) =
                type_table.get(some_type_at_some_point.name.as_str())
            {
                // The only scenario this is valid is if the new node override the old one
                if some_type_at_some_point_node
                    .parent
                    .is_none_or(|x| x.absolute() != older_type_index.absolute())
                {
                    r.push(DuplicateTypeDefinition {
                        name: some_type_at_some_point.name.to_string(),
                        offending_event: some_type_at_some_point_node.e,
                    });
                }
            }
        }

        let type_table = lc.types(window).collect::<BTreeMap<_, _>>();

        // Check that override types exist
        // TODO I'm not sure whether this should be an error or not.
        // What it's causing at the moment is that interrelated Node_Types have to
        // complete a ritual to allow overriding each othe. I'm demoting this to
        // a warning for now.
        for (ty_event, ty) in type_table.values() {
            for override_ty in ty.additional_override.iter() {
                if !type_table.contains_key(override_ty) {
                    r.push(NonexistantOverrideType {
                        offending_event: *ty_event,
                        base_ty_name: ty.name.to_string(),
                        override_ty_name: override_ty.to_string(),
                        relevance: vec![*ty_event],
                    })
                }
            }
        }

        // Check nodes for their fields

        for pointed_context in window
            .items()
            .filter(|e| !lc.is_stale(*e).unwrap() && lc.get(*e).is_some_and(|x| x.is_node()))
        {
            let node = lc.node(pointed_context).unwrap();
            if node.ty.is_none() {
                r.push(NodeTypeNonexistant {
                    offending_event: pointed_context,
                    ty_name: node.ty_name.to_string(),
                });
                continue;
            }
            let node_type = node.ty.unwrap();

            // Fields that may be present and mandatory
            for (field_name, (field_type, field_policy)) in node_type.field.iter() {
                match field_policy {
                NodeTypeFieldStatus::Mandatory => {
                    if !node.field.0.contains_key(field_name) {
                        r.push(ElidedMandatoryField {
                            offending_event: pointed_context,
                            ty_name: node_type.name.to_string(),
                            field_name: field_name.to_string(),
                        });
                    }
                }
                NodeTypeFieldStatus::Encouraged => {
                    if !node.field.0.contains_key(field_name) {
                        r.push(ElidedEncouragedField {
                            offending_event: pointed_context,
                            ty_name: node_type.name.to_string(),
                            field_name: field_name.to_string(),
                        });
                    }
                }
                NodeTypeFieldStatus::Deprecated => {
                    if node.field.0.contains_key(field_name) {
                        r.push(DeprecatedField {
                            offending_event: pointed_context,
                            ty_name: node_type.name.to_string(),
                            field_name: field_name.to_string(),
                        });
                    }
                }
                NodeTypeFieldStatus::Optional => {
                    // Nothing to do.
                }
                }
                if let Some(field_payload) = node.field.0.get(field_name)
                    && field_payload.discriminant() != *field_type
                {
                    r.push(FieldTypeMismatch {
                        offending_event: pointed_context,
                        ty_name: node_type.name.to_string(),
                        field_name: field_name.to_string(),
                        expected_type: *field_type,
                        present_type: field_payload.discriminant(),
                    })
                }
            }

            // Extraneous fields
            for extra_field in node.field.0.keys().filter(|body_field| {
                !node_type
                    .field
                    .iter()
                    .map(|x| x.0.as_str())
                    .contains(body_field.as_str())
            }) {
                r.push(ExtraneousField {
                    offending_event: pointed_context,
                    ty_name: node_type.name.to_string(),
                    field_name: extra_field.to_string(),
                })
            }
        }

        // Check edges ends so that they allow for that edge
        for pointed_context in window
            .items()
            .filter(|e| !lc.is_stale(*e).unwrap() && lc.get(*e).is_some_and(|x| x.is_edge()))
        {
            // from end -> "outgoing" relative to that node
            // to end -> "incoming" relative to that node
            let edge = lc.edge(pointed_context).unwrap();
            let outgoing_node = lc.node(edge.from).unwrap();
            let incoming_node = lc.node(edge.to).unwrap();

            if !(type_table.contains_key(outgoing_node.ty_name.as_str())
                && type_table.contains_key(incoming_node.ty_name.as_str()))
            {
                // We have bigger problems
                assert!(r.0.iter().any(|x| {
                    match x {
                    NodeTypeNonexistant { ty_name, .. } => [
                        outgoing_node.ty_name.as_str(),
                        incoming_node.ty_name.as_str(),
                    ]
                    .contains(&ty_name.as_str()),
                    _ => false,
                    }
                }));
                continue;
            }

            if !outgoing_node
                .ty
                .unwrap()
                .outgoing
                .iter()
                .map(String::as_str)
                .contains(edge.ty_name.as_str())
            {
                r.push(BadEdgeEndpoint {
                    offending_event: edge.e,
                    edge_ty_name: edge.ty_name.to_string(),
                    bad_node_ty_name: outgoing_node.ty_name.to_string(),
                    bad_node_is_head: true,
                });
            }
            // COPY_PASTE
            if !incoming_node
                .ty
                .unwrap()
                .incoming
                .iter()
                .map(String::as_str)
                .contains(edge.ty_name.as_str())
            {
                r.push(BadEdgeEndpoint {
                    offending_event: edge.e,
                    edge_ty_name: edge.ty_name.to_string(),
                    bad_node_ty_name: incoming_node.ty_name.to_string(),
                    bad_node_is_head: false,
                });
            }
        }

        // Check whether a child is allowed to override parent at the point of override
        for last_event_in_context in lc.window(window.items()) {
            let preceding = last_event_in_context.ctx().shrink_to_exclusive_left();
            if last_event_in_context.is_edge()
                || last_event_in_context.is_node_and(|x| x.parent.is_none())
            {
                continue;
            }

            let child_type_name = last_event_in_context.as_node().ty_name.clone();
            let parent_in_context =
                preceding.look_at(last_event_in_context.as_node().parent.unwrap());
            let Some(parent_type) = lc.node(parent_in_context).unwrap().ty else {
                // We have bigger problems.
                continue;
            };

            if child_type_name.as_str() != parent_type.name.as_str()
                && !parent_type.additional_override.contains(&child_type_name)
            {
                r.push(BadOverride {
                    offending_event: last_event_in_context.ctx(),
                    parent_ty_name: parent_type.name.to_string(),
                    child_ty_name: child_type_name.to_string(),
                });
            }
        }

        // Check Node Classes
        for fresh_node in lc.nodes(window).filter(|x| !lc.is_stale(x.e).unwrap()) {
            use lifecore::NodeTypeClass::*;
            let Some(ty) = fresh_node.ty else {
                // We have bigger problems
                continue;
            };
            let outgoing_count = lc.fresh_outgoing_edges_of(fresh_node.e).count();
            let incoming_count = lc.fresh_incoming_edges_to(fresh_node.e).count();
            // TODO dry
            match ty.class {
            Spike => {
                if incoming_count > 0 {
                    r.push(NodeClassNotRespected {
                        offending_event: fresh_node.e,
                        ty_name: ty.name.to_string(),
                        expected_class: Spike,
                    })
                }
            }
            Hub => {
                if outgoing_count > 0 {
                    r.push(NodeClassNotRespected {
                        offending_event: fresh_node.e,
                        ty_name: ty.name.to_string(),
                        expected_class: Hub,
                    })
                }
            }
            Toothpick => {
                if outgoing_count > 1 || incoming_count > 0 {
                    r.push(NodeClassNotRespected {
                        offending_event: fresh_node.e,
                        ty_name: ty.name.to_string(),
                        expected_class: Toothpick,
                    })
                }
            }
            EdgeNode => {
                if outgoing_count > 1 || incoming_count > 1 {
                    r.push(NodeClassNotRespected {
                        offending_event: fresh_node.e,
                        ty_name: ty.name.to_string(),
                        expected_class: EdgeNode,
                    })
                }
            }
            Regular => continue, // Anything goes
            }
        }

        r
    }

    fn convert_report(&self, r: TypeCheckOutput) -> analysis::Output
    where
        Self: Sized,
    {
        analysis::Output(
            r.0.into_iter()
                .map(|x| self.convert_message(x))
                .collect_vec(),
        )
    }

    fn initialize(&mut self, lc: &LC) -> analysis::Output {
        let x = self.initialize_inner(lc);
        self.convert_report(x)
    }

    fn analyze(&self, lc: &LC, window: EventContext) -> analysis::Output {
        self.convert_report(self.analyze_inner(lc, window))
    }
}

impl<LC: LifeCore + 'static> analysis::AnalyzerFactory<LC> for TypeCheckFactory<LC> {
    fn analyzer_with_config(
        &mut self,
        config: &str,
    ) -> Result<Box<dyn analysis::Analyzer<LC>>, LifeCoreError> {
        Ok(Box::new(TypeCheck {
            config: config.to_string(),
            _lc: PhantomData,
        }))
    }

    fn analyzer_name(&self) -> &'static str {
        Self::name()
    }
}

// Misc
impl<LC: LifeCore> Default for TypeCheckFactory<LC> {
    fn default() -> Self {
        Self {
            _x: Default::default(),
        }
    }
}

#[cfg(test)]
mod test;

/// Tests written by Agents
#[cfg(test)]
mod test2;
