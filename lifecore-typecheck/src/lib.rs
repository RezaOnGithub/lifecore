use lifecore::{
    Entry, EntryId, LifeCoreError, Node, View,
    analysis::{
        Analyzer, AnalyzerCertificate, AnalyzerFactory, Diagnostic, EvaluationInputs,
        InnerMessageType, Message, MessageKind, MessageType, TypedAnalyzer,
    },
    typing::{
        FieldIssue, FieldIssueKind, ResolutionError, SchemaError, Type, TypeDefinition, kind_word,
        path_text,
    },
};
use semver::Version;
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub enum TypeCheckMessage {
    MalformedDeclaration {
        subject: EntryId,
        error: SchemaError,
    },
    ReservedTypeName {
        subject: EntryId,
    },
    DuplicateType {
        subject: EntryId,
        name: String,
    },
    MissingType {
        subject: EntryId,
        name: String,
    },
    UnknownSuccessorType {
        subject: EntryId,
        name: String,
    },
    Field {
        subject: EntryId,
        issue: FieldIssue,
    },
    BadEdgeEndpoint {
        subject: EntryId,
        edge_type: String,
        node_type: String,
        outgoing: bool,
    },
    BadSuccessor {
        subject: EntryId,
        parent_type: String,
        child_type: String,
    },
}

impl InnerMessageType for TypeCheckMessage {
    fn ty(&self) -> MessageType {
        match self {
        Self::UnknownSuccessorType { .. } => MessageType::Warning,
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
    fn subject(&self) -> EntryId {
        match self {
        Self::MalformedDeclaration { subject, .. }
        | Self::ReservedTypeName { subject }
        | Self::DuplicateType { subject, .. }
        | Self::MissingType { subject, .. }
        | Self::UnknownSuccessorType { subject, .. }
        | Self::Field { subject, .. }
        | Self::BadEdgeEndpoint { subject, .. }
        | Self::BadSuccessor { subject, .. } => *subject,
        }
    }
    // Presentation only: the certificate, not this text, identifies a finding,
    // and the text names no boundary so re-emissions merge.
    fn description(&self) -> String {
        match self {
        Self::MalformedDeclaration { error, .. } => error.to_string(),
        Self::ReservedTypeName { .. } => {
            format!(
                "the name {:?} is reserved for the built-in metatype",
                Type::TYPE_NAME
            )
        }
        Self::DuplicateType { name, .. } => {
            format!("more than one active declaration names {name:?}")
        }
        Self::MissingType { name, .. } => format!("type {name:?} is not declared in this view"),
        Self::UnknownSuccessorType { name, .. } => {
            format!("permitted successor type {name:?} is not declared")
        }
        Self::Field { issue, .. } => {
            let path = path_text(&issue.path);
            match &issue.kind {
            FieldIssueKind::MissingMandatory => format!("mandatory field {path} is missing"),
            FieldIssueKind::MissingEncouraged => format!("encouraged field {path} is missing"),
            FieldIssueKind::Deprecated => format!("deprecated field {path} is present"),
            FieldIssueKind::UnexpectedField => format!("field {path} is not declared"),
            FieldIssueKind::TypeMismatch { expected, actual } => format!(
                "field {path} should be {}, found {}",
                kind_word(*expected),
                kind_word(*actual)
            ),
            }
        }
        Self::BadEdgeEndpoint {
            edge_type,
            node_type,
            outgoing,
            ..
        } => format!(
            "type {node_type:?} does not permit {} edge {edge_type:?}",
            if *outgoing { "outgoing" } else { "incoming" }
        ),
        Self::BadSuccessor {
            parent_type,
            child_type,
            ..
        } => format!("type {parent_type:?} does not permit successor {child_type:?}"),
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
        // A later schema cannot revoke a past transition. Each successor is judged
        // once, at the prefix it ends, against the view immediately before its
        // append. The runner replays every prefix, so every transition in history
        // is still judged, and its certificate omits the evaluated boundary. This
        // relies on every transition finding being an inconsistency, which current
        // reports select at any boundary; a nonsticky advisory judged only here
        // would drop out of later current reports.
        if let Some(child) = view.entries().next_back().and_then(Entry::as_node) {
            output.extend(Self::transition(view, child));
        }
        Ok(output)
    }
    fn convert_message(&self, finding: Self::MSG) -> Message {
        let subject = finding.subject();
        Message {
            // The typed variant carries its exact subject and distinguishing
            // details but no boundary, so identity spans prefixes.
            analyzer_certificate: AnalyzerCertificate(
                serde_json::to_string(&finding).expect("typed TypeCheck finding"),
            ),
            diagnostic: Diagnostic {
                subjects: [subject].into(),
                focus: Some(subject),
                message: finding.description(),
                kind: match finding.ty() {
                MessageType::Warning => MessageKind::Warning,
                _ => MessageKind::Inconsistency,
                },
                sticky: false,
            },
        }
    }
}

impl TypeCheck {
    // Declarations, missing types, fields, and edge endpoints of the current
    // graph in `view`.
    fn current_state(view: View<'_>) -> Vec<TypeCheckMessage> {
        use TypeCheckMessage::*;
        let mut output = Vec::new();
        let mut types = BTreeMap::<String, (EntryId, Type)>::new();
        for node in view.fresh_nodes().filter(|n| n.ty_name == Type::TYPE_NAME) {
            match Type::from_body(&node.payload) {
            Err(error) => output.push(MalformedDeclaration {
                subject: node.id,
                error,
            }),
            Ok(ty) if ty.name == Type::TYPE_NAME => {
                output.push(ReservedTypeName { subject: node.id })
            }
            Ok(ty) => {
                if types.contains_key(&ty.name) {
                    output.push(DuplicateType {
                        subject: node.id,
                        name: ty.name.clone(),
                    });
                }
                // NOTDO: No ambiguity tracking here yet. DuplicateType rejects the
                // candidate, but retaining the last declaration makes subsequent body
                // and edge findings depend on declaration order. Track ambiguous names
                // separately to suppress dependent checks without reporting MissingType.
                types.insert(ty.name.clone(), (node.id, ty));
            }
            }
        }
        for (id, ty) in types.values() {
            for name in &ty.successors {
                if name != Type::TYPE_NAME && !types.contains_key(name) {
                    output.push(UnknownSuccessorType {
                        subject: *id,
                        name: name.clone(),
                    });
                }
            }
        }
        for node in view.fresh_nodes().filter(|n| n.ty_name != Type::TYPE_NAME) {
            let Some((_, ty)) = types.get(&node.ty_name) else {
                output.push(MissingType {
                    subject: node.id,
                    name: node.ty_name.clone(),
                });
                continue;
            };
            output.extend(ty.check(&node.payload).into_iter().map(|issue| Field {
                subject: node.id,
                issue,
            }));
        }
        for edge in view.fresh_edges() {
            for (endpoint, outgoing) in [(edge.from, true), (edge.to, false)] {
                let node = view.node(endpoint).expect("validated endpoint");
                // Built-in Type has no ordinary edge permissions. Missing declarations
                // already have diagnostics.
                let allowed = if node.ty_name == Type::TYPE_NAME {
                    false
                } else if let Some((_, ty)) = types.get(&node.ty_name) {
                    (if outgoing { &ty.outgoing } else { &ty.incoming }).contains(&edge.ty_name)
                } else {
                    continue;
                };
                if !allowed {
                    output.push(BadEdgeEndpoint {
                        subject: edge.id,
                        edge_type: edge.ty_name.clone(),
                        node_type: node.ty_name.clone(),
                        outgoing,
                    });
                }
            }
        }
        output
    }
    // Judges a successor against the view immediately before its append; the
    // finding's subject is the child.
    fn transition(view: View<'_>, child: &Node) -> Option<TypeCheckMessage> {
        use TypeCheckMessage::*;
        let subject = child.id;
        let before = view
            .prefix(child.id.position())
            .expect("child is inside view");
        let predecessor = before
            .node(child.predecessor?)
            .expect("validated predecessor");
        let allowed = match before.resolve_type(&predecessor.ty_name) {
        Ok(Some(TypeDefinition::Builtin)) => child.ty_name == Type::TYPE_NAME,
        Ok(Some(TypeDefinition::Declared { schema, .. })) => {
            child.ty_name == schema.name || schema.successors.contains(&child.ty_name)
        }
        Ok(None) => {
            return Some(MissingType {
                subject,
                name: predecessor.ty_name.clone(),
            });
        }
        Err(ResolutionError::Malformed { error, .. }) => {
            return Some(MalformedDeclaration { subject, error });
        }
        Err(ResolutionError::Ambiguous { name, .. }) => {
            return Some(DuplicateType { subject, name });
        }
        };
        (!allowed).then(|| BadSuccessor {
            subject,
            parent_type: predecessor.ty_name.clone(),
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
    fn analyzer_with_config(
        &mut self,
        configuration: &str,
    ) -> Result<Box<dyn Analyzer>, LifeCoreError> {
        if !configuration.trim().is_empty()
            && !serde_json::from_str::<BTreeMap<String, serde_json::Value>>(configuration)
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
