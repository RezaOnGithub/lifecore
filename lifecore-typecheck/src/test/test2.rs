use crate::{TypeCheck, TypeCheckFactory, TypeCheckMessage};
use lifecore::{
    Body, EntryId, LifeCoreList, Payload,
    analysis::{AnalysisRunner, Analyzer, AnalyzerFactory, InnerMessageType, MessageType},
    transaction::{
        Change,
        Reference::{Added, Existing},
        prepare,
    },
    typing::{FieldDecl, FieldIssueKind, FieldStatus, Type, TypeDefinition, ValueType},
};

fn ty(name: &str) -> Type {
    Type::new(name, &[], &[], &[], &[])
}
fn declaration(ty: &Type, parent: Option<lifecore::transaction::Reference>) -> Change {
    Change::AddNode {
        ty_name: "Type".into(),
        body: ty.to_body(),
        parent,
    }
}
fn node(name: &str, body: Body, parent: Option<lifecore::transaction::Reference>) -> Change {
    Change::AddNode {
        ty_name: name.into(),
        body,
        parent,
    }
}
fn append(graph: &mut LifeCoreList, changes: &[Change]) -> Vec<EntryId> {
    let plan = prepare(graph, changes, &mut AnalysisRunner::default()).unwrap();
    let ids = plan.added().to_vec();
    plan.commit(graph).unwrap();
    ids
}
fn findings(graph: &LifeCoreList, end: usize) -> Vec<TypeCheckMessage> {
    TypeCheck::default()
        .analyze_inner(graph.view().prefix(end).unwrap())
        .0
}
fn has_error(findings: &[TypeCheckMessage]) -> bool {
    findings
        .iter()
        .any(|f| f.ty() == MessageType::Inconsistency)
}
fn configure() -> Change {
    Change::AddAnalyzer {
        name: TypeCheckFactory::name().into(),
        config: String::new(),
    }
}
fn runner() -> AnalysisRunner {
    let mut r = AnalysisRunner::default();
    r.register_analyzer_kind(Box::new(TypeCheckFactory))
        .unwrap();
    r
}

#[test]
fn builtin_metatype_needs_no_bootstrap_and_cannot_be_redeclared() {
    let mut analyzer = TypeCheck::default();
    assert!(analyzer.initialize().0.is_empty());
    let mut graph = LifeCoreList::default();
    assert!(findings(&graph, 0).is_empty());
    assert_eq!(
        graph.view().resolve_type("Type").unwrap(),
        Some(TypeDefinition::Builtin)
    );
    append(&mut graph, &[declaration(&ty("Type"), None)]);
    assert!(matches!(
        findings(&graph, 1)[0],
        TypeCheckMessage::ReservedTypeName { .. }
    ));
}

#[test]
fn malformed_declarations_return_typed_findings() {
    let mut graph = LifeCoreList::default();
    append(&mut graph, &[node("Type", Body::new(), None)]);
    assert!(matches!(
        findings(&graph, 1)[0],
        TypeCheckMessage::MalformedDeclaration { .. }
    ));
}

#[test]
fn missing_and_duplicate_declarations_are_inconsistencies() {
    let mut graph = LifeCoreList::default();
    append(&mut graph, &[node("Missing", Body::new(), None)]);
    assert!(matches!(
        findings(&graph, 1)[0],
        TypeCheckMessage::MissingType { .. }
    ));
    let mut a = ty("A");
    let first = append(&mut graph, &[declaration(&a, None)])[0];
    a.outgoing.insert("link".into());
    append(&mut graph, &[declaration(&a, None)]);
    assert!(findings(&graph,3).iter().any(|f| matches!(f,TypeCheckMessage::DuplicateType {location,..} if location.entry != Some(first))));
}

#[test]
fn replacing_a_declaration_uses_the_builtin_metatype() {
    let mut graph = LifeCoreList::default();
    let mut a = ty("A");
    let first = append(&mut graph, &[declaration(&a, None)])[0];
    a.outgoing.insert("link".into());
    append(&mut graph, &[declaration(&a, Some(Existing(first)))]);
    assert!(!has_error(&findings(&graph, 2)));
}

#[test]
fn unresolved_successor_types_are_advisory() {
    let mut graph = LifeCoreList::default();
    let mut a = ty("A");
    a.additional_override.insert("Future".into());
    append(&mut graph, &[declaration(&a, None)]);
    let report = findings(&graph, 1);
    assert!(!has_error(&report));
    assert!(matches!(
        report[0],
        TypeCheckMessage::UnknownOverrideType { .. }
    ));
}

#[test]
fn field_policies_and_shape_failures_remain_typed_and_separate() {
    let mut graph = LifeCoreList::default();
    let schema = Type::new(
        "A",
        &[
            ("must", (ValueType::Text, FieldStatus::Mandatory)),
            ("advice", (ValueType::Text, FieldStatus::Encouraged)),
            ("old", (ValueType::Text, FieldStatus::Deprecated)),
            ("optional", (ValueType::Boolean, FieldStatus::Optional)),
        ],
        &[],
        &[],
        &[],
    );
    append(
        &mut graph,
        &[
            declaration(&schema, None),
            node(
                "A",
                Body::from([
                    ("old".into(), "value".into()),
                    ("optional".into(), 1.into()),
                    ("extra".into(), true.into()),
                ]),
                None,
            ),
        ],
    );
    let issues = findings(&graph, 2)
        .into_iter()
        .filter_map(|f| {
            if let TypeCheckMessage::Field { issue, .. } = f {
                Some(issue.kind)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert!(issues.contains(&FieldIssueKind::MissingMandatory));
    assert!(issues.contains(&FieldIssueKind::MissingEncouraged));
    assert!(issues.contains(&FieldIssueKind::Deprecated));
    assert!(issues.contains(&FieldIssueKind::UnexpectedField));
    assert!(
        issues
            .iter()
            .any(|f| matches!(f, FieldIssueKind::TypeMismatch { .. }))
    );
}

#[test]
fn correctly_shaped_empty_collections_count_as_present() {
    let mut graph = LifeCoreList::default();
    let schema = Type::new(
        "A",
        &[
            (
                "list",
                (
                    ValueType::List(Box::new(ValueType::Text)),
                    FieldStatus::Mandatory,
                ),
            ),
            (
                "record",
                (
                    ValueType::Record(Default::default()),
                    FieldStatus::Encouraged,
                ),
            ),
        ],
        &[],
        &[],
        &[],
    );
    append(
        &mut graph,
        &[
            declaration(&schema, None),
            node(
                "A",
                Body::from([
                    ("list".into(), Payload::List(vec![])),
                    ("record".into(), Payload::Record(Body::new())),
                ]),
                None,
            ),
        ],
    );
    assert!(findings(&graph, 2).is_empty());
}

#[test]
fn wrong_empty_container_is_a_shape_error_not_a_missing_field() {
    let mut graph = LifeCoreList::default();
    let schema = Type::new(
        "A",
        &[(
            "list",
            (
                ValueType::List(Box::new(ValueType::Text)),
                FieldStatus::Mandatory,
            ),
        )],
        &[],
        &[],
        &[],
    );
    append(
        &mut graph,
        &[
            declaration(&schema, None),
            node(
                "A",
                Body::from([("list".into(), Payload::Record(Body::new()))]),
                None,
            ),
        ],
    );
    let report = findings(&graph, 2);
    assert_eq!(report.len(), 1);
    assert!(matches!(
        report[0],
        TypeCheckMessage::Field {
            issue: lifecore::typing::FieldIssue {
                kind: FieldIssueKind::TypeMismatch { .. },
                ..
            },
            ..
        }
    ));
}

#[test]
fn nested_records_and_scalar_lists_are_checked_recursively() {
    let mut graph = LifeCoreList::default();
    let shape = ValueType::Record(
        [(
            "flag".into(),
            FieldDecl {
                ty: ValueType::Boolean,
                status: FieldStatus::Mandatory,
            },
        )]
        .into(),
    );
    let schema = Type::new(
        "A",
        &[(
            "rows",
            (ValueType::List(Box::new(shape)), FieldStatus::Mandatory),
        )],
        &[],
        &[],
        &[],
    );
    append(
        &mut graph,
        &[
            declaration(&schema, None),
            node(
                "A",
                Body::from([(
                    "rows".into(),
                    Payload::List(vec![Payload::Record(Body::from([(
                        "flag".into(),
                        false.into(),
                    )]))]),
                )]),
                None,
            ),
        ],
    );
    assert!(findings(&graph, 2).is_empty());
}

#[test]
fn active_type_changes_reinterpret_only_fresh_nodes_in_the_selected_view() {
    let mut graph = LifeCoreList::default();
    let original = ty("A");
    let ids = append(
        &mut graph,
        &[declaration(&original, None), node("A", Body::new(), None)],
    );
    let changed = Type::new(
        "A",
        &[("required", (ValueType::Text, FieldStatus::Mandatory))],
        &[],
        &[],
        &[],
    );
    append(&mut graph, &[declaration(&changed, Some(Existing(ids[0])))]);
    assert!(!has_error(&findings(&graph, 2)));
    assert!(has_error(&findings(&graph, 3)));
    append(
        &mut graph,
        &[node(
            "A",
            Body::from([("required".into(), "value".into())]),
            Some(Existing(ids[1])),
        )],
    );
    assert!(!has_error(&findings(&graph, 4)));
    assert!(has_error(&findings(&graph, 3)));
}

#[test]
fn transition_permissions_are_checked_immediately_before_each_child() {
    let mut graph = LifeCoreList::default();
    let mut a = ty("A");
    a.additional_override.insert("B".into());
    let ids = append(
        &mut graph,
        &[
            declaration(&a, None),
            declaration(&ty("B"), None),
            node("A", Body::new(), None),
            node("B", Body::new(), Some(Added(2))),
        ],
    );
    a.additional_override.clear();
    append(&mut graph, &[declaration(&a, Some(Existing(ids[0])))]);
    assert!(!has_error(&findings(&graph, 5)));
    append(
        &mut graph,
        &[node(
            "B",
            Body::from([("different".into(), true.into())]),
            Some(Existing(ids[2])),
        )],
    );
    assert!(
        findings(&graph, 6)
            .iter()
            .any(|f| matches!(f, TypeCheckMessage::BadOverride { .. }))
    );
    assert!(
        !findings(&graph, 5)
            .iter()
            .any(|f| matches!(f, TypeCheckMessage::BadOverride { .. }))
    );
}

#[test]
fn branching_from_a_stale_parent_requires_its_type_to_still_resolve() {
    let mut graph = LifeCoreList::default();
    let ids = append(
        &mut graph,
        &[
            declaration(&ty("A"), None),
            node("A", Body::new(), None),
            node("A", Body::new(), Some(Added(1))),
        ],
    );
    append(
        &mut graph,
        &[
            declaration(&ty("B"), Some(Existing(ids[0]))),
            node(
                "A",
                Body::from([("new".into(), true.into())]),
                Some(Existing(ids[1])),
            ),
        ],
    );
    assert!(findings(&graph,5).iter().any(|f| matches!(f,TypeCheckMessage::MissingType { location,.. } if location.entry == graph.view().at(4).map(|e|e.id()))));
}

#[test]
fn both_endpoint_permissions_are_checked_in_the_selected_view() {
    for (outgoing, incoming) in [(true, true), (false, true), (true, false)] {
        let mut graph = LifeCoreList::default();
        let a = Type::new("A", &[], if outgoing { &["link"] } else { &[] }, &[], &[]);
        let b = Type::new("B", &[], &[], if incoming { &["link"] } else { &[] }, &[]);
        append(
            &mut graph,
            &[
                declaration(&a, None),
                declaration(&b, None),
                node("A", Body::new(), None),
                node("B", Body::new(), None),
                Change::AddEdge {
                    from: Added(2),
                    ty_name: "link".into(),
                    to: Added(3),
                },
            ],
        );
        let report = findings(&graph, 5);
        assert_eq!(has_error(&report), !(outgoing && incoming));
        if !outgoing {
            assert!(
                report
                    .iter()
                    .any(|f| matches!(f, TypeCheckMessage::BadEdgeEndpoint { outgoing: true, .. }))
            );
        }
        if !incoming {
            assert!(report.iter().any(|f| matches!(
                f,
                TypeCheckMessage::BadEdgeEndpoint {
                    outgoing: false,
                    ..
                }
            )));
        }
    }
}

#[test]
fn configured_typechecking_rejects_an_invalid_prefix_even_after_a_repair() {
    let mut graph = LifeCoreList::default();
    let a = Type::new(
        "A",
        &[("field", (ValueType::Text, FieldStatus::Mandatory))],
        &[],
        &[],
        &[],
    );
    let changes = [
        declaration(&a, None),
        node("A", Body::new(), None),
        node(
            "A",
            Body::from([("field".into(), "value".into())]),
            Some(Added(1)),
        ),
        configure(),
    ];
    assert!(prepare(&graph, &changes, &mut runner()).is_err());
    assert!(graph.view().is_empty());
    prepare(
        &graph,
        &[
            declaration(&a, None),
            node("A", Body::from([("field".into(), "value".into())]), None),
            configure(),
        ],
        &mut runner(),
    )
    .unwrap()
    .commit(&mut graph)
    .unwrap();
    assert_eq!(graph.view().len(), 2);
}

#[test]
fn factory_validates_configuration_and_keeps_its_name() {
    let mut factory = TypeCheckFactory;
    assert_eq!(factory.analyzer_name(), "Type_Check");
    assert!(factory.analyzer_with_config("unexpected").is_err());
    assert!(factory.analyzer_with_config("{}").is_ok());
}

#[test]
fn runner_combines_typed_warnings_and_inconsistencies_without_filtering() {
    let mut graph = LifeCoreList::default();
    let a = Type::new(
        "A",
        &[
            ("required", (ValueType::Text, FieldStatus::Mandatory)),
            ("advised", (ValueType::Text, FieldStatus::Encouraged)),
        ],
        &[],
        &[],
        &[],
    );
    append(
        &mut graph,
        &[declaration(&a, None), node("A", Body::new(), None)],
    );
    let suite = lifecore::analysis::SuiteConfig(vec![lifecore::analysis::Config {
        name: TypeCheckFactory::name().into(),
        config: String::new(),
    }]);
    let output = runner().run_view(graph.view(), &suite).unwrap();
    assert_eq!(output.errors().count(), 1);
    assert_eq!(output.warnings().count(), 1);
    assert_eq!(output.errors().next().unwrap().offending_event.view.end, 2);
}

#[test]
fn plan_local_references_count_operations_and_cannot_name_configuration_changes() {
    let graph = LifeCoreList::default();
    let changes = [
        configure(),
        declaration(&ty("A"), None),
        node("A", Body::new(), None),
        node("A", Body::new(), Some(Added(2))),
    ];
    let plan = prepare(&graph, &changes, &mut runner()).unwrap();
    assert_eq!(plan.added().len(), 3);
    assert_eq!(
        plan.view().node(plan.added()[2]).unwrap().parent,
        Some(plan.added()[1])
    );
    assert!(matches!(
        prepare(
            &graph,
            &[configure(), node("A", Body::new(), Some(Added(0)))],
            &mut runner()
        ),
        Err(lifecore::transaction::Error::InvalidPlanReference { operation: 0 })
    ));
}
