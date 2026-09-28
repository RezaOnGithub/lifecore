use lifecore::{
    DiagnosticLocation, Entry, EntryId, LifeCoreError, Node, View,
    analysis::{
        Analyzer, AnalyzerCertificate, AnalyzerFactory, EvaluationInputs, InnerMessageType,
        Message, MessageKind, MessageType, TypedAnalyzer,
    },
    typing::{FieldIssue, FieldIssueKind, ResolutionError, SchemaError, Type, TypeDefinition},
};
use semver::Version;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum TypeCheckMessage {
    MalformedDeclaration {
        #[serde(skip)]
        location: DiagnosticLocation,
        error: SchemaError,
    },
    ReservedTypeName {
        #[serde(skip)]
        location: DiagnosticLocation,
    },
    DuplicateType {
        #[serde(skip)]
        location: DiagnosticLocation,
        name: String,
    },
    MissingType {
        #[serde(skip)]
        location: DiagnosticLocation,
        name: String,
    },
    UnknownOverrideType {
        #[serde(skip)]
        location: DiagnosticLocation,
        name: String,
    },
    Field {
        #[serde(skip)]
        location: DiagnosticLocation,
        issue: FieldIssue,
    },
    BadEdgeEndpoint {
        #[serde(skip)]
        location: DiagnosticLocation,
        edge_type: String,
        node_type: String,
        outgoing: bool,
    },
    BadOverride {
        #[serde(skip)]
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

#[derive(Default)]
pub struct TypeCheck {}

impl TypedAnalyzer for TypeCheck {
    type MSG = TypeCheckMessage;
    fn version(&self) -> Version {
        Version::parse(env!("CARGO_PKG_VERSION")).expect("valid package version")
    }
    fn initialize_inner(&mut self, _inputs: &EvaluationInputs) -> Result<Vec<Self::MSG>, String> {
        Ok(Vec::new())
    }
    fn analyze_inner(
        &self,
        view: View<'_>,
        _inputs: &EvaluationInputs,
    ) -> Result<Vec<Self::MSG>, String> {
        let mut output = Self::current_state(view);
        //A A later schema cannot revoke a past transition. Each successor is judged
        //A once, at the prefix it ends, against the view immediately before its
        //A append. The runner replays every prefix, so every transition in history
        //A is still judged, and its certificate omits the evaluated boundary. This
        //A relies on every transition finding being an inconsistency, which current
        //A reports select at any boundary; a nonsticky advisory judged only here
        //A would drop out of later current reports.
        if let Some(child) = view.entries().next_back().and_then(Entry::as_node) {
            output.extend(Self::transition(view, child));
        }
        Ok(output)
    }
    fn convert_message(&self, finding: Self::MSG) -> Message {
        let location = finding.location();
        let message = finding.description();
        Message {
            //A The typed variant skips its ordinary boundary; the exact
            //A subject and distinguishing details still define identity.
            analyzer_certificate: AnalyzerCertificate(
                serde_json::to_string(&(&finding, location.entry))
                    .expect("typed TypeCheck finding"),
            ),
            subjects: location.entry.into_iter().collect(),
            offending_event: Some(location),
            relevance: location.entry.into_iter().collect(),
            message,
            kind: match finding.ty() {
            MessageType::Warning => MessageKind::Warning,
            _ => MessageKind::Inconsistency,
            },
            sticky: false,
        }
    }
}

impl TypeCheck {
    //A Declarations, missing types, fields, and edge endpoints of the current
    //A graph in `view`.
    fn current_state(view: View<'_>) -> Vec<TypeCheckMessage> {
        use TypeCheckMessage::*;
        let location = |id| view.location(Some(id)).expect("visible diagnostic subject");
        let mut output = Vec::new();
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
            output.extend(ty.check(&node.body).into_iter().map(|issue| Field {
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
        output
    }
    //A Judges a successor against the view immediately before its append; the
    //A finding is located at the child in `view`.
    fn transition(view: View<'_>, child: &Node) -> Option<TypeCheckMessage> {
        use TypeCheckMessage::*;
        let location = view
            .location(Some(child.id))
            .expect("visible diagnostic subject");
        let before = view
            .prefix(child.id.position())
            .expect("child is inside view");
        let parent = before.node(child.parent?).expect("validated parent");
        let allowed = match before.resolve_type(&parent.ty_name) {
        Ok(Some(TypeDefinition::Builtin)) => child.ty_name == Type::TYPE_NAME,
        Ok(Some(TypeDefinition::Declared { schema, .. })) => {
            child.ty_name == schema.name || schema.additional_override.contains(&child.ty_name)
        }
        Ok(None) => {
            return Some(MissingType {
                location,
                name: parent.ty_name.clone(),
            });
        }
        Err(ResolutionError::Malformed { error, .. }) => {
            return Some(MalformedDeclaration { location, error });
        }
        Err(ResolutionError::Ambiguous { name, .. }) => {
            return Some(DuplicateType { location, name });
        }
        };
        (!allowed).then(|| BadOverride {
            location,
            parent_type: parent.ty_name.clone(),
            child_type: child.ty_name.clone(),
        })
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
        if !config.trim().is_empty()
            && !serde_json::from_str::<BTreeMap<String, serde_json::Value>>(config)
                .is_ok_and(|fields| fields.is_empty())
        {
            return Err(LifeCoreError::InvalidAnalyzerConfiguration {
                analyzer_name: Self::name().into(),
                message: "TypeCheck accepts only an empty configuration".into(),
            });
        }
        Ok(Box::new(TypeCheck::default()))
    }
}

#[cfg(test)]
mod test;
