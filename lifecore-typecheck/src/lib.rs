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
    Body, Context, EntryContext, EntryIndex, LifeCore, LifeCoreError, OrderedSet, Payload,
    PayloadKind, analysis::InnerMessageType,
};
use std::{collections::BTreeMap, fmt, marker::PhantomData, str::FromStr};
use strum::IntoDiscriminant;

/// Describes whether a field is mandatory or not.
///
/// A field can be:
/// - Mandatory; must be filled out
/// - Encouraged: should be filled out but could remain non-existent for now.
///   The field may become mandatory in the future.
/// - Deprecated: should not be filled out but could remain filled out for now.
///   The field may be removed in the future.
/// - Optional: may or may not be filled out
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub enum FieldStatus {
    Mandatory,
    Encouraged,
    Deprecated,
    Optional,
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
    OrderedSet(Box<ValueType>),
    OneOf(Vec<ValueType>),
}

#[derive(Clone, PartialEq, Eq, Hash)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub struct FieldDecl {
    name: String,
    ty: ValueType,
    status: FieldStatus,
}

/// Describes a type in LifeCore
///
/// Types operate primarily on Nodes and describe their fields, the
/// edge types that can be connected to them, and what other node types (besides
/// their own) can override them.
///
/// Types for nodes are defined by nodes! In other words, the type information
/// also resides on the database.
///
/// To achieve this, we define implement [`NodeType`] for [`Type`] node type,
/// which will be a node that can be added to the database like any other node.
#[derive(Clone, PartialEq, Eq, Hash)]
#[cfg_attr(debug_assertions, derive(Debug))]
pub struct Type {
    pub name: String,
    pub body: OrderedSet<FieldDecl>,
    pub outgoing: OrderedSet<String>,
    pub incoming: OrderedSet<String>,
    pub additional_override: OrderedSet<String>,
}

/// A struct that implements this can easily be adapted to insert and retrieve
/// data from LifeCore
pub trait NodeType<'a>: Into<Body> + TryFrom<&'a Body> {
    fn describe_self() -> Type;
    fn name() -> &'static str;
}

/// A trait for NodeType implementors. If every field of a struct implements
/// this, implementing NodeType becomes easier too- the only missing pieces are
/// field name and field status.
pub trait NodeField<'a>: Into<Payload> + TryFrom<&'a Payload> {
    fn describe_self() -> ValueType;
}

/// Describes a Field as a whole
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
        offending_event: EntryContext,
    },
    DuplicateTypeDefinition {
        offending_event: EntryContext,
        name: String,
    },
    NonexistantOverrideType {
        offending_event: EntryContext,
        base_ty_name: String,
        override_ty_name: String,
        relevance: Vec<EntryIndex>,
    },
    NodeTypeNonexistant {
        offending_event: EntryContext,
        ty_name: String,
    },
    ElidedMandatoryField {
        offending_event: EntryContext,
        ty_name: String,
        field_name: String,
    },
    // TODO add relevance
    ElidedEncouragedField {
        offending_event: EntryContext,
        ty_name: String,
        field_name: String,
    },
    // TODO add relevance
    DeprecatedField {
        offending_event: EntryContext,
        ty_name: String,
        field_name: String,
    },
    FieldTypeMismatch {
        offending_event: EntryContext,
        ty_name: String,
        field_name: String,
        expected_type: PayloadKind,
        present_type: PayloadKind,
    },
    ExtraneousField {
        offending_event: EntryContext,
        ty_name: String,
        field_name: String,
    },
    BadEdgeEndpoint {
        offending_event: EntryContext,
        edge_ty_name: String,
        bad_node_ty_name: String,
        bad_node_is_head: bool,
    },
    BadOverride {
        offending_event: EntryContext,
        parent_ty_name: String,
        child_ty_name: String,
    },
}

pub fn types<LC: LifeCore>(
    lc: &LC,
    ctx: Context,
) -> impl Iterator<Item = (String, (EntryContext, Type))> {
    lc.fresh_nodes(ctx)
        .filter(|n| n.ty_name.as_str() == Type::name())
        .filter_map(|n| {
            let ty: Type = (&n.field).try_into().ok()?;
            Some((ty.name.clone(), (n.e, ty)))
        })
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
    fn ty(&self) -> lifecore::analysis::MessageType {
        use lifecore::analysis::MessageType::*;
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
        }
    }
}

impl<LC: LifeCore> TypeCheck<LC> {
    fn convert_message(&self, x: TypeCheckMessage) -> lifecore::analysis::Message {
        use lifecore::analysis::{Inconsistency, SuggestedAction::*, Suggestion, Warning};
        let system_name = TypeCheckFactory::<LC>::name().to_string();
        let system_config = self.config.to_string();
        let r: lifecore::analysis::Message = match x.clone() {
        TypeCheckMessage::AddBootstrapNode => Suggestion {
            offending_event: Context::universe().into(),
            system_name: TypeCheckFactory::<LC>::name().to_string(),
            system_config: self.config.to_string(),
            relevance: vec![],
            message: "Consider adding the bootstrap node for TypeCheck to work".to_string(),
            action: vec![AddNode {
                ty_name: Type::name().to_string(),
                payload: Type::describe_self().into(),
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
        };
        assert!(r.discriminant() == x.ty());
        r
    }
}

impl lifecore::analysis::InnerOutput<TypeCheckMessage> for TypeCheckOutput {
    fn push(&mut self, v: TypeCheckMessage) {
        self.0.push(v);
    }
}

impl<LC: LifeCore> lifecore::analysis::Analyzer<LC> for TypeCheck<LC> {
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
        if types(lc, Context::universe()).any(|(name, _)| name == Type::name()) {
            return TypeCheckOutput(vec![]);
        }

        TypeCheckOutput(vec![TypeCheckMessage::AddBootstrapNode])
    }

    fn analyze_inner(&self, lc: &LC, window: Context) -> TypeCheckOutput
    where
        Self: Sized,
    {
        use TypeCheckMessage::*;
        let mut r = TypeCheckOutput(vec![]);
        // check whether a Node_Type declaring some type X is not clashing with an older
        // but non-stale Node_Type declaring a type of the same name.
        for some_type_at_some_point_node in lc
            .nodes(window)
            .filter(|node| node.ty_name.as_str() == Type::name())
        {
            let type_table = types(
                lc,
                some_type_at_some_point_node.e.shrink_to_exclusive_left(),
            )
            .collect::<BTreeMap<_, _>>();

            let Ok(some_type_at_some_point) = Type::try_from(&some_type_at_some_point_node.field)
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

        let type_table = types(lc, window).collect::<BTreeMap<_, _>>();

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
                        relevance: vec![ty_event.entry()],
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
            let Some((_type_event, _node_type)) = type_table.get(node.ty_name.as_str()) else {
                r.push(NodeTypeNonexistant {
                    offending_event: pointed_context,
                    ty_name: node.ty_name.to_string(),
                });
                continue;
            };

            // TODO: Check fields against the resolved type's FieldDecl values.
        }

        // Check edges ends so that they allow for that edge
        for pointed_context in window
            .items()
            .filter(|e| !lc.is_stale(*e).unwrap() && lc.get(*e).is_some_and(|x| x.is_edge()))
        {
            // from end -> "outgoing" relative to that node
            // to end -> "incoming" relative to that node
            let edge = lc.edge(pointed_context).unwrap();
            let outgoing_node = lc.node(edge.e.look_at(edge.from.absolute())).unwrap();
            let incoming_node = lc.node(edge.e.look_at(edge.to.absolute())).unwrap();

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

            if !type_table
                .get(outgoing_node.ty_name.as_str())
                .unwrap()
                .1
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
            if !type_table
                .get(incoming_node.ty_name.as_str())
                .unwrap()
                .1
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
            let entry_context = last_event_in_context.entry_context();
            let preceding = entry_context.shrink_to_exclusive_left();
            if last_event_in_context.is_edge()
                || last_event_in_context.is_node_and(|x| x.parent.is_none())
            {
                continue;
            }

            let child_type_name = last_event_in_context.as_node().ty_name.clone();
            let parent_in_context =
                preceding.look_at(last_event_in_context.as_node().parent.unwrap().absolute());
            let parent_node = lc.node(parent_in_context).unwrap();
            let preceding_type_table = types(lc, preceding).collect::<BTreeMap<_, _>>();
            let Some((_type_event, parent_type)) =
                preceding_type_table.get(parent_node.ty_name.as_str())
            else {
                // We have bigger problems.
                continue;
            };

            if child_type_name.as_str() != parent_type.name.as_str()
                && !parent_type.additional_override.contains(&child_type_name)
            {
                r.push(BadOverride {
                    offending_event: entry_context,
                    parent_ty_name: parent_type.name.to_string(),
                    child_ty_name: child_type_name.to_string(),
                });
            }
        }

        r
    }

    fn convert_report(&self, r: TypeCheckOutput) -> lifecore::analysis::Output
    where
        Self: Sized,
    {
        lifecore::analysis::Output(
            r.0.into_iter()
                .map(|x| self.convert_message(x))
                .collect_vec(),
        )
    }

    fn initialize(&mut self, lc: &LC) -> lifecore::analysis::Output {
        let x = self.initialize_inner(lc);
        self.convert_report(x)
    }

    fn analyze(&self, lc: &LC, window: Context) -> lifecore::analysis::Output {
        self.convert_report(self.analyze_inner(lc, window))
    }
}

impl<LC: LifeCore + 'static> lifecore::analysis::AnalyzerFactory<LC> for TypeCheckFactory<LC> {
    fn analyzer_with_config(
        &mut self,
        config: &str,
    ) -> Result<Box<dyn lifecore::analysis::Analyzer<LC>>, LifeCoreError> {
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

impl FromStr for ValueType {
    type Err = LifeCoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // VALUE_TYPE := PRIMITIVE
        //             | ('Sequence' | 'OrderedSet') '(' VALUE_TYPE ')'
        //             | ('Tuple' | 'OneOf') '(' (VALUE_TYPE (',' VALUE_TYPE)*)? ')'
        fn error() -> LifeCoreError {
            LifeCoreError::NodeFieldDeserializationError {
                target_type: Some("ValueType".to_string()),
                field_name: None,
            }
        }

        fn skip_whitespace(s: &str, cursor: &mut usize) {
            while let Some(c) = s[*cursor..].chars().next()
                && c.is_whitespace()
            {
                *cursor += c.len_utf8();
            }
        }

        fn consume(s: &str, cursor: &mut usize, expected: char) -> bool {
            skip_whitespace(s, cursor);
            if s[*cursor..].starts_with(expected) {
                *cursor += expected.len_utf8();
                true
            } else {
                false
            }
        }

        fn expect(s: &str, cursor: &mut usize, expected: char) -> Result<(), LifeCoreError> {
            if consume(s, cursor, expected) {
                Ok(())
            } else {
                Err(error())
            }
        }

        fn keyword<'a>(s: &'a str, cursor: &mut usize) -> Result<&'a str, LifeCoreError> {
            skip_whitespace(s, cursor);
            let start = *cursor;
            while let Some(c) = s[*cursor..].chars().next()
                && c.is_ascii_alphabetic()
            {
                *cursor += c.len_utf8();
            }
            if start == *cursor {
                Err(error())
            } else {
                Ok(&s[start..*cursor])
            }
        }

        fn type_list(s: &str, cursor: &mut usize) -> Result<Vec<ValueType>, LifeCoreError> {
            let mut values = vec![];
            if consume(s, cursor, ')') {
                return Ok(values);
            }
            loop {
                values.push(value_type(s, cursor)?);
                if consume(s, cursor, ')') {
                    return Ok(values);
                }
                expect(s, cursor, ',')?;
            }
        }

        fn value_type(s: &str, cursor: &mut usize) -> Result<ValueType, LifeCoreError> {
            use ValueType::*;
            match keyword(s, cursor)? {
            "PreciseTime" | "precisetime" => Ok(PreciseTime),
            "Text" | "text" => Ok(Text),
            "Number" | "number" => Ok(Number),
            "Sequence" | "sequence" => {
                expect(s, cursor, '(')?;
                let inner = value_type(s, cursor)?;
                expect(s, cursor, ')')?;
                Ok(Sequence(Box::new(inner)))
            }
            "OrderedSet" | "orderedset" => {
                expect(s, cursor, '(')?;
                let inner = value_type(s, cursor)?;
                expect(s, cursor, ')')?;
                Ok(OrderedSet(Box::new(inner)))
            }
            "Tuple" | "tuple" => {
                expect(s, cursor, '(')?;
                Ok(Tuple(type_list(s, cursor)?))
            }
            "OneOf" | "oneof" => {
                expect(s, cursor, '(')?;
                Ok(OneOf(type_list(s, cursor)?))
            }
            _ => Err(error()),
            }
        }

        let mut cursor = 0;
        let parsed = value_type(s, &mut cursor)?;
        skip_whitespace(s, &mut cursor);
        if cursor == s.len() {
            Ok(parsed)
        } else {
            Err(error())
        }
    }
}

impl fmt::Display for ValueType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn list(f: &mut fmt::Formatter<'_>, values: &[ValueType]) -> fmt::Result {
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    f.write_str(", ")?;
                }
                write!(f, "{value}")?;
            }
            Ok(())
        }

        match self {
        ValueType::PreciseTime => f.write_str("PreciseTime"),
        ValueType::Text => f.write_str("Text"),
        ValueType::Number => f.write_str("Number"),
        ValueType::Sequence(inner) => write!(f, "Sequence({inner})"),
        ValueType::Tuple(values) => {
            f.write_str("Tuple(")?;
            list(f, values)?;
            f.write_str(")")
        }
        ValueType::OrderedSet(inner) => write!(f, "OrderedSet({inner})"),
        ValueType::OneOf(values) => {
            f.write_str("OneOf(")?;
            list(f, values)?;
            f.write_str(")")
        }
        }
    }
}

impl FromStr for FieldDecl {
    type Err = LifeCoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // FIELD_DECL := FIELD_NAME '=' VALUE_TYPE ':' FIELD_STATUS
        fn error() -> LifeCoreError {
            LifeCoreError::NodeFieldDeserializationError {
                target_type: Some("FieldDecl".to_string()),
                field_name: None,
            }
        }

        fn skip_whitespace(s: &str, cursor: &mut usize) {
            while let Some(c) = s[*cursor..].chars().next()
                && c.is_whitespace()
            {
                *cursor += c.len_utf8();
            }
        }

        fn consume(s: &str, cursor: &mut usize, expected: char) -> bool {
            skip_whitespace(s, cursor);
            if s[*cursor..].starts_with(expected) {
                *cursor += expected.len_utf8();
                true
            } else {
                false
            }
        }

        fn expect(s: &str, cursor: &mut usize, expected: char) -> Result<(), LifeCoreError> {
            if consume(s, cursor, expected) {
                Ok(())
            } else {
                Err(error())
            }
        }

        fn field_name(s: &str, cursor: &mut usize) -> Result<String, LifeCoreError> {
            skip_whitespace(s, cursor);
            let start = *cursor;
            let Some(first) = s[*cursor..].chars().next() else {
                return Err(error());
            };
            if !first.is_ascii_lowercase() {
                return Err(error());
            }
            *cursor += first.len_utf8();
            while let Some(c) = s[*cursor..].chars().next()
                && (c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            {
                *cursor += c.len_utf8();
            }
            Ok(s[start..*cursor].to_string())
        }

        fn value_type<'a>(s: &'a str, cursor: &mut usize) -> Result<&'a str, LifeCoreError> {
            skip_whitespace(s, cursor);
            let start = *cursor;
            let mut depth = 0usize;
            while let Some(c) = s[*cursor..].chars().next() {
                match c {
                '(' => depth += 1,
                ')' => {
                    depth = depth.checked_sub(1).ok_or_else(error)?;
                }
                ':' if depth == 0 => break,
                _ => {}
                }
                *cursor += c.len_utf8();
            }
            if start == *cursor || depth != 0 {
                Err(error())
            } else {
                Ok(s[start..*cursor].trim())
            }
        }

        fn status(s: &str, cursor: &mut usize) -> Result<FieldStatus, LifeCoreError> {
            skip_whitespace(s, cursor);
            let start = *cursor;
            while let Some(c) = s[*cursor..].chars().next()
                && c.is_ascii_alphabetic()
            {
                *cursor += c.len_utf8();
            }
            match &s[start..*cursor] {
            "Mandatory" | "mandatory" => Ok(FieldStatus::Mandatory),
            "Encouraged" | "encouraged" => Ok(FieldStatus::Encouraged),
            "Deprecated" | "deprecated" => Ok(FieldStatus::Deprecated),
            "Optional" | "optional" => Ok(FieldStatus::Optional),
            _ => Err(error()),
            }
        }

        let mut cursor = 0;
        let name = field_name(s, &mut cursor)?;
        expect(s, &mut cursor, '=')?;
        let ty = value_type(s, &mut cursor)?
            .parse::<ValueType>()
            .map_err(|_| error())?;
        expect(s, &mut cursor, ':')?;
        let status = status(s, &mut cursor)?;
        skip_whitespace(s, &mut cursor);
        if cursor != s.len() {
            return Err(error());
        }
        Ok(Self { name, ty, status })
    }
}

impl fmt::Display for FieldDecl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = match self.status {
        FieldStatus::Mandatory => "Mandatory",
        FieldStatus::Encouraged => "Encouraged",
        FieldStatus::Deprecated => "Deprecated",
        FieldStatus::Optional => "Optional",
        };
        write!(f, "{} = {} : {status}", self.name, self.ty)
    }
}

mod boilerplate;
#[cfg(test)]
mod test;
