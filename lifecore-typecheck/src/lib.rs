use lifecore::{
    DiagnosticLocation, EntryId, LifeCoreError, View,
    analysis::{
        Analyzer, AnalyzerFactory, Inconsistency, InnerMessageType, InnerOutput, Message,
        MessageType, Output, Warning,
    },
    typing::{FieldIssue, FieldIssueKind, ResolutionError, SchemaError, Type, TypeDefinition},
};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum TypeCheckMessage {
    MalformedDeclaration {
        location: DiagnosticLocation,
        error: SchemaError,
    },
    ReservedTypeName {
        location: DiagnosticLocation,
    },
    DuplicateType {
        location: DiagnosticLocation,
        name: String,
    },
    MissingType {
        location: DiagnosticLocation,
        name: String,
    },
    UnknownOverrideType {
        location: DiagnosticLocation,
        name: String,
    },
    Field {
        location: DiagnosticLocation,
        issue: FieldIssue,
    },
    BadEdgeEndpoint {
        location: DiagnosticLocation,
        edge_type: String,
        node_type: String,
        outgoing: bool,
    },
    BadOverride {
        location: DiagnosticLocation,
        parent_type: String,
        child_type: String,
    },
}

impl InnerMessageType for TypeCheckMessage {
    fn ty(&self) -> MessageType {
        match self {
        Self::UnknownOverrideType { .. } => MessageType::Warning,
        Self::Field { issue, .. }
            if matches!(
                issue.kind,
                FieldIssueKind::MissingEncouraged | FieldIssueKind::Deprecated
            ) =>
        {
            MessageType::Warning
        }
        _ => MessageType::Inconsistency,
        }
    }
}

impl TypeCheckMessage {
    fn location(&self) -> DiagnosticLocation {
        match self {
        Self::MalformedDeclaration { location, .. }
        | Self::ReservedTypeName { location }
        | Self::DuplicateType { location, .. }
        | Self::MissingType { location, .. }
        | Self::UnknownOverrideType { location, .. }
        | Self::Field { location, .. }
        | Self::BadEdgeEndpoint { location, .. }
        | Self::BadOverride { location, .. } => *location,
        }
    }
    fn description(&self) -> String {
        match self {
        Self::MalformedDeclaration { error, .. } => error.to_string(),
        Self::ReservedTypeName { .. } => {
            "Type is a fixed built-in metatype and cannot be declared".into()
        }
        Self::DuplicateType { name, .. } => {
            format!("More than one active declaration names {name:?}")
        }
        Self::MissingType { name, .. } => format!("Type {name:?} is not declared in this view"),
        Self::UnknownOverrideType { name, .. } => {
            format!("Permitted successor type {name:?} is not declared")
        }
        Self::Field { issue, .. } => format!("Field {:?}: {:?}", issue.path, issue.kind),
        Self::BadEdgeEndpoint {
            edge_type,
            node_type,
            outgoing,
            ..
        } => format!(
            "Type {node_type:?} does not permit {} edge {edge_type:?}",
            if *outgoing { "outgoing" } else { "incoming" }
        ),
        Self::BadOverride {
            parent_type,
            child_type,
            ..
        } => format!("Type {parent_type:?} does not permit successor {child_type:?}"),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct TypeCheckOutput(pub Vec<TypeCheckMessage>);
impl InnerOutput<TypeCheckMessage> for TypeCheckOutput {
    fn push(&mut self, v: TypeCheckMessage) {
        self.0.push(v);
    }
}

#[derive(Default)]
pub struct TypeCheck {
    config: String,
}

impl Analyzer for TypeCheck {
    type MSG = TypeCheckMessage;
    type OUTPUT = TypeCheckOutput;
    fn initialize_inner(&mut self) -> Self::OUTPUT {
        TypeCheckOutput::default()
    }
    fn analyze_inner(&self, view: View<'_>) -> Self::OUTPUT {
        use TypeCheckMessage::*;
        let location = |id| view.location(Some(id)).expect("visible diagnostic subject");
        let mut output = TypeCheckOutput::default();
        let mut types = BTreeMap::<String, (EntryId, Type)>::new();
        for node in view.fresh_nodes().filter(|n| n.ty_name == Type::TYPE_NAME) {
            match Type::from_body(&node.body) {
            Err(error) => output.push(MalformedDeclaration {
                location: location(node.id),
                error,
            }),
            Ok(ty) if ty.name == Type::TYPE_NAME => output.push(ReservedTypeName {
                location: location(node.id),
            }),
            Ok(ty) => {
                if types.contains_key(&ty.name) {
                    output.push(DuplicateType {
                        location: location(node.id),
                        name: ty.name.clone(),
                    });
                }
                //A NOTDO: No ambiguity tracking here yet. DuplicateType rejects the
                //A candidate, but retaining the last declaration makes subsequent body
                //A and edge findings depend on declaration order. Track ambiguous names
                //A separately to suppress dependent checks without reporting MissingType.
                types.insert(ty.name.clone(), (node.id, ty));
            }
            }
        }
        for (id, ty) in types.values() {
            for name in &ty.additional_override {
                if name != Type::TYPE_NAME && !types.contains_key(name) {
                    output.push(UnknownOverrideType {
                        location: location(*id),
                        name: name.clone(),
                    });
                }
            }
        }
        for node in view.fresh_nodes().filter(|n| n.ty_name != Type::TYPE_NAME) {
            let Some((_, ty)) = types.get(&node.ty_name) else {
                output.push(MissingType {
                    location: location(node.id),
                    name: node.ty_name.clone(),
                });
                continue;
            };
            output
                .0
                .extend(ty.check(&node.body).into_iter().map(|issue| Field {
                    location: location(node.id),
                    issue,
                }));
        }
        for edge in view.fresh_edges() {
            for (endpoint, outgoing) in [(edge.from, true), (edge.to, false)] {
                let node = view.node(endpoint).expect("validated endpoint");
                //A Built-in Type has no ordinary edge permissions. Missing declarations
                //A already have diagnostics.
                let allowed = if node.ty_name == Type::TYPE_NAME {
                    false
                } else if let Some((_, ty)) = types.get(&node.ty_name) {
                    (if outgoing { &ty.outgoing } else { &ty.incoming }).contains(&edge.ty_name)
                } else {
                    continue;
                };
                if !allowed {
                    output.push(BadEdgeEndpoint {
                        location: location(edge.id),
                        edge_type: edge.ty_name.clone(),
                        node_type: node.ty_name.clone(),
                        outgoing,
                    });
                }
            }
        }
        for child in view.nodes().filter(|n| n.parent.is_some()) {
            //A A later schema cannot revoke a past transition; each child is judged
            //A immediately before its append.
            let before = view
                .prefix(child.id.position())
                .expect("child is inside view");
            let parent = before
                .node(child.parent.expect("selected child"))
                .expect("validated parent");
            let allowed = match before.resolve_type(&parent.ty_name) {
            Ok(Some(TypeDefinition::Builtin)) => child.ty_name == Type::TYPE_NAME,
            Ok(Some(TypeDefinition::Declared { schema, .. })) => {
                child.ty_name == schema.name || schema.additional_override.contains(&child.ty_name)
            }
            Ok(None) => {
                output.push(MissingType {
                    location: location(child.id),
                    name: parent.ty_name.clone(),
                });
                continue;
            }
            Err(ResolutionError::Malformed { error, .. }) => {
                output.push(MalformedDeclaration {
                    location: location(child.id),
                    error,
                });
                continue;
            }
            Err(ResolutionError::Ambiguous { name, .. }) => {
                output.push(DuplicateType {
                    location: location(child.id),
                    name,
                });
                continue;
            }
            };
            if !allowed {
                output.push(BadOverride {
                    location: location(child.id),
                    parent_type: parent.ty_name.clone(),
                    child_type: child.ty_name.clone(),
                });
            }
        }
        output
    }
    fn convert_report(&self, report: Self::OUTPUT) -> Output {
        Output(
            report
                .0
                .into_iter()
                .map(|finding| {
                    let location = finding.location();
                    let message = finding.description();
                    match finding.ty() {
                    MessageType::Warning => Message::Warning(Warning {
                        offending_event: location,
                        system_name: TypeCheckFactory::name().into(),
                        system_config: self.config.clone(),
                        relevance: location.entry.into_iter().collect(),
                        message,
                    }),
                    _ => Message::Inconsistency(Inconsistency {
                        offending_event: location,
                        system_name: TypeCheckFactory::name().into(),
                        system_config: self.config.clone(),
                        message,
                    }),
                    }
                })
                .collect(),
        )
    }
    fn initialize(&mut self) -> Output {
        let report = self.initialize_inner();
        self.convert_report(report)
    }
    fn analyze(&self, view: View<'_>) -> Output {
        self.convert_report(self.analyze_inner(view))
    }
}

#[derive(Default)]
pub struct TypeCheckFactory;
impl TypeCheckFactory {
    pub fn name() -> &'static str {
        "Type_Check"
    }
}
impl AnalyzerFactory for TypeCheckFactory {
    fn analyzer_name(&self) -> &'static str {
        Self::name()
    }
    fn analyzer_with_config(&mut self, config: &str) -> Result<Box<dyn Analyzer>, LifeCoreError> {
        if !matches!(config.trim(), "" | "{}") {
            return Err(LifeCoreError::InvalidAnalyzerConfiguration {
                analyzer_name: Self::name().into(),
                message: "TypeCheck accepts only an empty configuration".into(),
            });
        }
        Ok(Box::new(TypeCheck {
            config: config.into(),
        }))
    }
}

#[cfg(test)]
mod test;
