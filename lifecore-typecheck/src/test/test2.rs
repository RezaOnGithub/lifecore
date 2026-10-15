use crate::{TypeCheck, TypeCheckFactory, TypeCheckMessage};
use lifecore::{
    Body, EntryId, LifeCoreError, LifeCoreList, Payload, PayloadKind, View,
    analysis::{
        AnalysisOutput, AnalysisRunner, Analyzer, AnalyzerFactory, Config, EvaluatedView,
        EvaluationInputs, FindingKey, InnerMessageType, Message, MessageType, SuiteConfig,
        TypedAnalyzer,
        presentation::{Dismissals, ReportOptions, ReportSelection},
    },
    transaction::{
        Change, EntryChange, NewEdge, NewNode,
        Reference::{Added, Existing},
        RevisionIntent, prepare,
    },
    typing::{
        FieldDecl, FieldIssueKind, FieldStatus, PathPart, ResolutionError, SchemaError,
        SchemaErrorKind, Type, TypeDefinition, ValueType,
    },
};

fn ty(name: &str) -> Type {
    Type::new(name, &[], &[], &[], &[])
}
fn declaration(ty: &Type, predecessor: Option<lifecore::transaction::Reference>) -> Change {
    Change::Entry(EntryChange::Node(NewNode {
        ty_name: "Type".into(),
        payload: ty.to_body(),
        predecessor,
        intent: predecessor.map(|_| RevisionIntent::Branch),
    }))
}
fn node(
    name: &str,
    payload: Body,
    predecessor: Option<lifecore::transaction::Reference>,
) -> Change {
    Change::Entry(EntryChange::Node(NewNode {
        ty_name: name.into(),
        payload,
        predecessor,
        intent: predecessor.map(|_| RevisionIntent::Branch),
    }))
}
fn append(graph: &mut LifeCoreList, changes: &[Change]) -> Vec<EntryId> {
    let plan = prepare(graph, changes, &mut AnalysisRunner::default())
        .unwrap()
        .accepted()
        .unwrap();
    let ids = plan.added().to_vec();
    plan.commit(graph).unwrap();
    ids
}
fn findings(graph: &LifeCoreList, end_exclusive: usize) -> Vec<TypeCheckMessage> {
    TypeCheck::default()
        .analyze_inner(
            graph.view().prefix(end_exclusive).unwrap(),
            &Default::default(),
        )
        .unwrap()
}
fn has_error(findings: &[TypeCheckMessage]) -> bool {
    findings
        .iter()
        .any(|f| f.ty() == MessageType::Inconsistency)
}
fn configure() -> Change {
    Change::AddAnalyzer {
        name: TypeCheckFactory::name().into(),
        configuration: String::new(),
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
    assert!(analyzer.initialize(&Default::default()).unwrap().is_empty());
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

// A declaration in the nested-envelope encoding `Type::to_body` produced
// before field declarations became word lists: one mandatory text field.
fn former_declaration(fields_key: &str, successors_key: &str) -> Body {
    let presence_and_shape = Payload::Record(Body::from([
        (
            "shape".into(),
            Payload::Record(Body::from([("kind".into(), "text".into())])),
        ),
        ("presence".into(), "mandatory".into()),
    ]));
    Body::from([
        ("name".into(), "A".into()),
        (
            fields_key.into(),
            Payload::Record(Body::from([("title".into(), presence_and_shape)])),
        ),
        ("outgoing".into(), Payload::List(vec![])),
        ("incoming".into(), Payload::List(vec![])),
        (successors_key.into(), Payload::List(vec![])),
    ])
}

#[test]
fn the_former_envelope_encoding_is_a_malformed_declaration() {
    use PathPart::Field;
    let cases = [
        (
            former_declaration("body", "additional_override"),
            SchemaError {
                path: vec![Field("additional_override".into())],
                kind: SchemaErrorKind::UnknownField,
            },
        ),
        // Even under the current keys, a per-field record is not a word list.
        (
            former_declaration("fields", "successors"),
            SchemaError {
                path: vec![Field("fields".into()), Field("title".into())],
                kind: SchemaErrorKind::WrongShape {
                    expected: PayloadKind::List,
                    actual: PayloadKind::Record,
                },
            },
        ),
    ];
    for (body, expected) in cases {
        let changes = [
            node("Type", body, None),
            node("A", Body::from([("title".into(), "x".into())]), None),
        ];
        let mut graph = LifeCoreList::default();
        let ids = append(&mut graph, &changes);
        let report = findings(&graph, 2);
        assert!(has_error(&report));
        assert!(report.iter().any(|f| matches!(
            f,
            TypeCheckMessage::MalformedDeclaration { subject, error }
                if *subject == ids[0] && *error == expected
        )));
        assert!(matches!(
            graph.view().resolve_type("A"),
            Err(ResolutionError::Malformed { id, error }) if id == ids[0] && error == expected
        ));
        // With TypeCheck configured, the declaration is rejected, not stored.
        let checked = prepare(
            &LifeCoreList::default(),
            &[configure(), changes[0].clone()],
            &mut runner(),
        )
        .unwrap();
        assert!(checked.analysis().has_error());
        assert!(checked.accepted().is_err());
    }
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
    assert!(findings(&graph, 3).iter().any(
        |f| matches!(f, TypeCheckMessage::DuplicateType { subject, .. } if *subject != first)
    ));
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
    a.successors.insert("Future".into());
    append(&mut graph, &[declaration(&a, None)]);
    let report = findings(&graph, 1);
    assert!(!has_error(&report));
    assert!(matches!(
        report[0],
        TypeCheckMessage::UnknownSuccessorType { .. }
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
    a.successors.insert("B".into());
    let ids = append(
        &mut graph,
        &[
            declaration(&a, None),
            declaration(&ty("B"), None),
            node("A", Body::new(), None),
            node("B", Body::new(), Some(Added(2))),
        ],
    );
    a.successors.clear();
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
            .any(|f| matches!(f, TypeCheckMessage::BadSuccessor { .. }))
    );
    assert!(
        !findings(&graph, 5)
            .iter()
            .any(|f| matches!(f, TypeCheckMessage::BadSuccessor { .. }))
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
    let revised = graph.view().at(4).unwrap().id();
    assert!(findings(&graph, 5).iter().any(
        |f| matches!(f, TypeCheckMessage::MissingType { subject, .. } if *subject == revised)
    ));
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
                Change::Entry(EntryChange::Edge(NewEdge {
                    from: Added(2),
                    ty_name: "link".into(),
                    to: Added(3),
                })),
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
    let checked = prepare(&graph, &changes, &mut runner()).unwrap();
    assert_eq!(
        checked
            .analysis()
            .errors()
            .map(|o| boundaries(o.evaluated_view))
            .collect::<Vec<_>>(),
        vec![Some((2, 2))]
    );
    assert!(checked.accepted().is_err());
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
    .accepted()
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
        configuration: String::new(),
    }]);
    let output = runner().run_view(graph.view(), &suite).unwrap();
    assert_eq!(output.errors().count(), 1);
    assert_eq!(output.warnings().count(), 1);
    assert_eq!(
        boundaries(output.errors().next().unwrap().evaluated_view),
        Some((2, 2))
    );
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
    let plan = prepare(&graph, &changes, &mut runner())
        .unwrap()
        .accepted()
        .unwrap();
    assert_eq!(plan.added().len(), 3);
    assert_eq!(
        plan.view().node(plan.added()[2]).unwrap().predecessor,
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

#[test]
fn normalized_field_findings_group_across_boundaries_and_survive_dismissal_replay() {
    use lifecore::analysis::{
        Config, SuiteConfig,
        presentation::{DismissalReason, Dismissals, ReportOptions, ReportSelection},
    };
    let mut graph = LifeCoreList::default();
    let schema = Type::new(
        "A",
        &[
            ("first", (ValueType::Text, FieldStatus::Encouraged)),
            ("second", (ValueType::Text, FieldStatus::Encouraged)),
        ],
        &[],
        &[],
        &[],
    );
    let ids = append(
        &mut graph,
        &[
            declaration(&schema, None),
            node("A", Body::new(), None),
            declaration(&ty("Other"), None),
        ],
    );
    let config = |raw: &str| {
        SuiteConfig(vec![Config {
            name: TypeCheckFactory::name().into(),
            configuration: raw.into(),
        }])
    };
    let report = runner().run_view(graph.view(), &config(" ")).unwrap();
    let mut dismissals = Dismissals::default();
    let groups = report.groups(&dismissals, ReportOptions::default());
    assert_eq!(groups.len(), 2);
    // Each warning holds at prefixes 2 and 3, so it is one observation.
    assert!(groups.iter().all(|g| {
        g.observations.len() == 1
            && g.selected.len() == 1
            && boundaries(g.observations[0].evaluated_view) == Some((2, 3))
    }));
    let key = groups[0].key.clone();
    // The subject is inside the typed finding; no boundary is.
    assert_eq!(
        certificate(&key),
        serde_json::json!({"Field": {"subject": ids[1],
            "issue": {"path": [{"Field": "first"}], "kind": "MissingEncouraged"}}})
    );
    assert_eq!(
        key.analyzer.version,
        env!("CARGO_PKG_VERSION")
            .parse::<semver::Version>()
            .unwrap()
    );
    assert_eq!(key.analyzer.configuration, " ");
    assert_ne!(key, *groups[1].key);
    dismissals
        .dismiss(
            key.clone(),
            DismissalReason::Acknowledged,
            Some("known field".into()),
        )
        .unwrap();
    let equivalent = runner().run_view(graph.view(), &config(" { } ")).unwrap();
    assert_ne!(report, equivalent);
    assert_eq!(
        report.warnings().next().unwrap().key.analyzer_certificate,
        equivalent
            .warnings()
            .next()
            .unwrap()
            .key
            .analyzer_certificate
    );
    assert_eq!(
        equivalent
            .groups(&dismissals, ReportOptions::default())
            .len(),
        2
    );
    let replay = runner().run_view(graph.view(), &config(" ")).unwrap();
    assert_eq!(report, replay);
    assert_eq!(
        replay.groups(&dismissals, ReportOptions::default()).len(),
        1
    );
    append(
        &mut graph,
        &[node(
            "A",
            Body::from([
                ("first".into(), "done".into()),
                ("second".into(), "done".into()),
            ]),
            Some(Existing(ids[1])),
        )],
    );
    let after = runner().run_view(graph.view(), &config(" ")).unwrap();
    assert!(
        after
            .groups(&dismissals, ReportOptions::default())
            .is_empty()
    );
    assert_eq!(
        after
            .groups(
                &dismissals,
                ReportOptions {
                    selection: ReportSelection::History,
                    include_dismissed: true,
                    ..Default::default()
                }
            )
            .len(),
        2
    );
    assert!(dismissals.restore(&key));
    // Revising the node at prefix 4 ends both ranges at prefix 3.
    assert_eq!(after.warnings().count(), 2);
    assert!(
        after
            .warnings()
            .all(|o| boundaries(o.evaluated_view) == Some((2, 3)))
    );
}

fn apply_checked(graph: &mut LifeCoreList, changes: &[Change]) -> Vec<EntryId> {
    let plan = prepare(graph, changes, &mut runner())
        .unwrap()
        .accepted()
        .unwrap();
    let ids = plan.added();
    plan.commit(graph).unwrap();
    ids
}
fn certificate(key: &FindingKey) -> serde_json::Value {
    serde_json::from_str(&key.analyzer_certificate.0).unwrap()
}
// The inclusive prefix boundaries; `None` at initialization.
fn boundaries(view: EvaluatedView) -> Option<(usize, usize)> {
    match view {
    EvaluatedView::Prefixes { first, last } => Some((first.end_exclusive, last.end_exclusive)),
    EvaluatedView::Initialization => None,
    }
}

#[test]
fn a_bad_transition_is_reported_once_at_its_own_prefix_and_rejects_the_plan() {
    let mut graph = LifeCoreList::default();
    let ids = apply_checked(
        &mut graph,
        &[
            configure(),
            declaration(&ty("A"), None),
            declaration(&ty("B"), None),
            node("A", Body::new(), None),
        ],
    );
    // Two further appends follow the bad transition at position 3.
    let checked = prepare(
        &graph,
        &[
            node("B", Body::new(), Some(Existing(ids[2]))),
            declaration(&ty("C"), None),
            node("C", Body::new(), None),
        ],
        &mut runner(),
    )
    .unwrap();
    let child = checked.operations()[0].unwrap();
    let analysis = checked.analysis();
    assert_eq!(analysis.view.end_exclusive, 6);
    let errors = analysis.errors().collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    // Judged once, at the prefix that appended the child, so the range does
    // not extend to the later boundaries.
    assert_eq!(boundaries(errors[0].evaluated_view), Some((4, 4)));
    assert_eq!(errors[0].diagnostic.focus, Some(child));
    assert_eq!(errors[0].diagnostic.subjects, [child].into());
    assert_eq!(
        certificate(&errors[0].key),
        serde_json::json!({"BadSuccessor": {"subject": child, "parent_type": "A", "child_type": "B"}})
    );
    // Inconsistencies are selected in a current report whatever their boundary.
    let dismissals = Dismissals::default();
    let groups = analysis.groups(&dismissals, ReportOptions::default());
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].key, &errors[0].key);
    assert_eq!(groups[0].selected, errors);
    assert!(checked.accepted().is_err());
    assert_eq!(graph.view().len(), 3);
}

#[test]
fn a_bad_transition_deep_in_history_still_blocks_configuring_typecheck() {
    let mut graph = LifeCoreList::default();
    let ids = append(
        &mut graph,
        &[
            declaration(&ty("A"), None),
            declaration(&ty("B"), None),
            node("A", Body::new(), None),
        ],
    );
    let child = append(
        &mut graph,
        &[
            node("B", Body::new(), Some(Existing(ids[2]))),
            declaration(&ty("C"), None),
            node("C", Body::new(), None),
        ],
    )[0];
    // The head view no longer judges the transition; replaying prefix 4 does.
    assert!(findings(&graph, 6).is_empty());
    let checked = prepare(&graph, &[configure()], &mut runner()).unwrap();
    let errors = checked.analysis().errors().collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    assert_eq!(boundaries(errors[0].evaluated_view), Some((4, 4)));
    assert_eq!(errors[0].diagnostic.subjects, [child].into());
    assert_eq!(
        certificate(&errors[0].key),
        serde_json::json!({"BadSuccessor": {"subject": child, "parent_type": "A", "child_type": "B"}})
    );
    assert!(checked.accepted().is_err());
}

#[test]
fn revising_a_declaration_later_does_not_revoke_an_earlier_transition() {
    let mut graph = LifeCoreList::default();
    let mut a = ty("A");
    a.successors.insert("B".into());
    let b = Type::new(
        "B",
        &[("n", (ValueType::Integer, FieldStatus::Optional))],
        &[],
        &[],
        &[],
    );
    let ids = apply_checked(
        &mut graph,
        &[
            configure(),
            declaration(&a, None),
            declaration(&b, None),
            node("A", Body::new(), None),
            node("B", Body::new(), Some(Added(3))),
        ],
    );
    a.successors.clear();
    let revoke = prepare(
        &graph,
        &[declaration(&a, Some(Existing(ids[0])))],
        &mut runner(),
    )
    .unwrap();
    assert!(!revoke.analysis().has_error());
    revoke.accepted().unwrap().commit(&mut graph).unwrap();
    // The recorded transition at position 3 stays valid under full replay, but
    // a new one is judged under the revised declaration.
    let accepted = runner().run(&graph).unwrap();
    assert!(!accepted.has_error());
    let checked = prepare(
        &graph,
        &[node(
            "B",
            Body::from([("n".into(), 1.into())]),
            Some(Existing(ids[2])),
        )],
        &mut runner(),
    )
    .unwrap();
    let child = checked.operations()[0].unwrap();
    let errors = checked.analysis().errors().collect::<Vec<_>>();
    assert_eq!(errors.len(), 1);
    assert_eq!(boundaries(errors[0].evaluated_view), Some((6, 6)));
    assert_eq!(
        certificate(&errors[0].key),
        serde_json::json!({"BadSuccessor": {"subject": child, "parent_type": "A", "child_type": "B"}})
    );
    assert!(checked.accepted().is_err());
}

#[test]
fn analyze_inner_judges_only_the_newest_entry_of_the_selected_prefix() {
    let mut graph = LifeCoreList::default();
    let mut a = ty("A");
    a.successors.insert("B".into());
    let ids = append(
        &mut graph,
        &[
            declaration(&a, None),
            declaration(&ty("B"), None),
            node("A", Body::new(), None),
        ],
    );
    a.successors.clear();
    let child = append(
        &mut graph,
        &[
            declaration(&a, Some(Existing(ids[0]))),
            node("B", Body::new(), Some(Existing(ids[2]))),
            declaration(&ty("C"), None),
        ],
    )[1];
    let bad_successor = |end_exclusive| {
        findings(&graph, end_exclusive)
            .into_iter()
            .filter(|f| matches!(f, TypeCheckMessage::BadSuccessor { .. }))
            .collect::<Vec<_>>()
    };
    assert!(bad_successor(4).is_empty());
    let at_own_prefix = bad_successor(5);
    assert_eq!(at_own_prefix.len(), 1);
    assert!(matches!(
        &at_own_prefix[0],
        TypeCheckMessage::BadSuccessor { subject, parent_type, child_type }
            if *subject == child && parent_type == "A" && child_type == "B"
    ));
    assert!(bad_successor(6).is_empty());
    // The former loop judged it again at every later prefix.
    let reference = AllChildrenTypeCheck
        .analyze_inner(graph.view(), &Default::default())
        .unwrap();
    assert!(reference.iter().any(|f| matches!(
        f,
        TypeCheckMessage::BadSuccessor { subject, .. } if *subject == child
    )));
    // Emitted once, the finding's range stays at its own prefix in every
    // later view; only identical re-emissions would extend it.
    let suite = SuiteConfig(vec![Config {
        name: TypeCheckFactory::name().into(),
        configuration: String::new(),
    }]);
    for end_exclusive in [5, 6] {
        let output = runner()
            .run_view(graph.view().prefix(end_exclusive).unwrap(), &suite)
            .unwrap();
        let errors = output.errors().collect::<Vec<_>>();
        assert_eq!(errors.len(), 1);
        assert_eq!(boundaries(errors[0].evaluated_view), Some((5, 5)));
        assert_eq!(errors[0].diagnostic.focus, Some(child));
    }
}

#[test]
fn a_warning_that_holds_until_the_head_is_one_observation_per_subject() {
    let task = Type::new(
        "Task",
        &[
            ("title", (ValueType::Text, FieldStatus::Optional)),
            ("notes", (ValueType::Text, FieldStatus::Encouraged)),
        ],
        &[],
        &[],
        &[],
    );
    let mut changes = vec![configure(), declaration(&task, None)];
    changes.extend((0..20).map(|i| {
        node(
            "Task",
            Body::from([("title".into(), format!("task {i}").into())]),
            None,
        )
    }));
    let graph = LifeCoreList::default();
    let checked = prepare(&graph, &changes, &mut runner()).unwrap();
    let tasks = checked.operations()[2..]
        .iter()
        .map(|id| id.unwrap())
        .collect::<Vec<_>>();
    let analysis = checked.analysis();
    assert_eq!(analysis.view.end_exclusive, 21);
    assert!(!analysis.has_error());
    // Without merging, task i would have one observation per later prefix.
    assert_eq!(analysis.observations().count(), 20);
    let warnings = analysis.warnings().collect::<Vec<_>>();
    assert_eq!(warnings.len(), 20);
    for (warning, task) in warnings.iter().zip(&tasks) {
        assert_eq!(warning.diagnostic.focus, Some(*task));
        assert_eq!(
            boundaries(warning.evaluated_view),
            Some((task.position() + 1, 21))
        );
    }
    let committed = {
        let mut graph = graph;
        checked.accepted().unwrap().commit(&mut graph).unwrap();
        graph
    };
    // An earlier view ends every range at its own boundary.
    let suite = SuiteConfig(vec![Config {
        name: TypeCheckFactory::name().into(),
        configuration: String::new(),
    }]);
    let output = runner()
        .run_view(committed.view().prefix(11).unwrap(), &suite)
        .unwrap();
    let ranges = output
        .warnings()
        .map(|o| boundaries(o.evaluated_view))
        .collect::<Vec<_>>();
    assert_eq!(
        ranges,
        tasks[..10]
            .iter()
            .map(|task| Some((task.position() + 1, 11)))
            .collect::<Vec<_>>()
    );
}

#[test]
fn transitions_from_ambiguous_types_and_from_declarations_have_typed_findings() {
    let mut graph = LifeCoreList::default();
    let mut other_a = ty("A");
    other_a.outgoing.insert("link".into());
    let child = append(
        &mut graph,
        &[
            declaration(&ty("A"), None),
            declaration(&other_a, None),
            node("A", Body::new(), None),
            node("A", Body::from([("v".into(), 1.into())]), Some(Added(2))),
        ],
    )[3];
    assert!(findings(&graph, 4).iter().any(|f| matches!(
        f,
        TypeCheckMessage::DuplicateType { subject, name }
            if *subject == child && name == "A"
    )));

    // The built-in metatype permits only declaration successors.
    let mut graph = LifeCoreList::default();
    let child = append(
        &mut graph,
        &[
            declaration(&ty("A"), None),
            declaration(&ty("B"), None),
            node("A", Body::new(), Some(Added(1))),
        ],
    )[2];
    let report = findings(&graph, 3);
    assert!(report.iter().any(|f| matches!(
        f,
        TypeCheckMessage::BadSuccessor { subject, parent_type, child_type }
            if *subject == child && parent_type == "Type" && child_type == "A"
    )));
}

// The transition loop from a07483e, verbatim apart from the later `successors`,
// `BadSuccessor`, and `subject` changes, and independent of
// `TypeCheck::transition`: every child in the view is judged against the
// prefix before it, at every prefix containing it.
fn all_children_transitions(view: View<'_>) -> Vec<TypeCheckMessage> {
    use TypeCheckMessage::*;
    let mut output = Vec::new();
    for child in view.nodes().filter(|n| n.predecessor.is_some()) {
        // A later schema cannot revoke a past transition; each child is judged
        // immediately before its append.
        let before = view
            .prefix(child.id.position())
            .expect("child is inside view");
        let parent = before
            .node(child.predecessor.expect("selected child"))
            .expect("validated parent");
        let allowed = match before.resolve_type(&parent.ty_name) {
        Ok(Some(TypeDefinition::Builtin)) => child.ty_name == Type::TYPE_NAME,
        Ok(Some(TypeDefinition::Declared { schema, .. })) => {
            child.ty_name == schema.name || schema.successors.contains(&child.ty_name)
        }
        Ok(None) => {
            output.push(MissingType {
                subject: child.id,
                name: parent.ty_name.clone(),
            });
            continue;
        }
        Err(ResolutionError::Malformed { error, .. }) => {
            output.push(MalformedDeclaration {
                subject: child.id,
                error,
            });
            continue;
        }
        Err(ResolutionError::Ambiguous { name, .. }) => {
            output.push(DuplicateType {
                subject: child.id,
                name,
            });
            continue;
        }
        };
        if !allowed {
            output.push(BadSuccessor {
                subject: child.id,
                parent_type: parent.ty_name.clone(),
                child_type: child.ty_name.clone(),
            });
        }
    }
    output
}

// The former analyzer: unchanged current-state checks plus the loop above.
struct AllChildrenTypeCheck;

impl TypedAnalyzer for AllChildrenTypeCheck {
    type MSG = TypeCheckMessage;
    fn version(&self) -> semver::Version {
        TypedAnalyzer::version(&TypeCheck::default())
    }
    fn initialize_inner(&mut self, _inputs: &EvaluationInputs) -> Result<Vec<Self::MSG>, String> {
        Ok(Vec::new())
    }
    fn analyze_inner(
        &self,
        view: View<'_>,
        _inputs: &EvaluationInputs,
    ) -> Result<Vec<Self::MSG>, String> {
        let mut output = TypeCheck::current_state(view);
        output.extend(all_children_transitions(view));
        Ok(output)
    }
    fn convert_message(&self, finding: Self::MSG) -> Message {
        TypeCheck::default().convert_message(finding)
    }
}

// Registered under TypeCheck's name and version, so finding keys compare
// directly with TypeCheck's own.
struct AllChildrenFactory;

impl AnalyzerFactory for AllChildrenFactory {
    fn analyzer_name(&self) -> &'static str {
        TypeCheckFactory::name()
    }
    fn analyzer_with_config(&mut self, config: &str) -> Result<Box<dyn Analyzer>, LifeCoreError> {
        TypeCheckFactory.analyzer_with_config(config)?;
        Ok(Box::new(AllChildrenTypeCheck))
    }
}

// Each finding key with the boundary that first emitted it, in first-emission
// order; `None` is initialization.
fn first_emissions(output: &AnalysisOutput) -> Vec<(&FindingKey, Option<usize>)> {
    let mut first = Vec::<(&FindingKey, Option<usize>)>::new();
    for observation in output.observations() {
        if !first.iter().any(|(key, _)| *key == &observation.key) {
            let at = boundaries(observation.evaluated_view).map(|(first, _)| first);
            first.push((&observation.key, at));
        }
    }
    first
}

// The number of (observation, prefix) emissions an output covers.
fn covered(output: &AnalysisOutput) -> usize {
    output
        .observations()
        .filter_map(|o| boundaries(o.evaluated_view))
        .map(|(first, last)| last - first + 1)
        .sum()
}

fn assert_replay_matches_all_children(graph: &LifeCoreList) -> usize {
    let suite = SuiteConfig(vec![Config {
        name: TypeCheckFactory::name().into(),
        configuration: String::new(),
    }]);
    let mut reference_runner = AnalysisRunner::default();
    reference_runner
        .register_analyzer_kind(Box::new(AllChildrenFactory))
        .unwrap();
    let mut repeated = 0;
    for end_exclusive in 0..=graph.view().len() {
        let view = graph.view().prefix(end_exclusive).unwrap();
        // At the typed level, each prefix yields the current-state findings plus
        // the former loop's judgment of the newest entry alone.
        let newest_entry = view.entries().next_back().map(|entry| entry.id());
        let mut expected = TypeCheck::current_state(view);
        expected.extend(
            all_children_transitions(view)
                .into_iter()
                .filter(|finding| Some(finding.subject()) == newest_entry),
        );
        assert_eq!(
            TypeCheck::default()
                .analyze_inner(view, &Default::default())
                .unwrap(),
            expected,
            "boundary {end_exclusive}"
        );
        let newest = runner().run_view(view, &suite).unwrap();
        let reference = reference_runner.run_view(view, &suite).unwrap();
        assert_eq!(first_emissions(&newest), first_emissions(&reference));
        assert_eq!(newest.has_error(), reference.has_error());
        // Newest-only replay drops only repeated judgments of older children,
        // which the former loop's ranges extend over.
        assert!(newest.observations().all(|o| {
            reference.observations().any(|r| {
                r.key == o.key
                    && r.diagnostic == o.diagnostic
                    && match (boundaries(r.evaluated_view), boundaries(o.evaluated_view)) {
                    (Some((r_first, r_last)), Some((o_first, o_last))) => {
                        r_first <= o_first && o_last <= r_last
                    }
                    (r, o) => r == o,
                    }
            })
        }));
        for selection in [ReportSelection::Current, ReportSelection::History] {
            let options = ReportOptions {
                selection,
                ..Default::default()
            };
            let keys = |output: &AnalysisOutput| {
                output
                    .groups(&Dismissals::default(), options)
                    .into_iter()
                    .map(|g| g.key.clone())
                    .collect::<Vec<_>>()
            };
            assert_eq!(keys(&newest), keys(&reference));
        }
        repeated = covered(&reference) - covered(&newest);
    }
    repeated
}

#[test]
fn newest_only_transitions_match_the_all_children_loop_under_full_replay() {
    let with_field = |name: &str| {
        Type::new(
            name,
            &[("v", (ValueType::Integer, FieldStatus::Optional))],
            &[],
            &[],
            &[],
        )
    };
    let v = |n: i64| Body::from([("v".into(), n.into())]);
    let mut graphs = vec![];

    // A valid transition, a revoked permission, then a bad transition and a
    // same-type revision of the valid child.
    let mut graph = LifeCoreList::default();
    let mut a = with_field("A");
    a.successors.insert("B".into());
    let ids = append(
        &mut graph,
        &[
            declaration(&a, None),
            declaration(&with_field("B"), None),
            node("A", Body::new(), None),
            node("B", Body::new(), Some(Added(2))),
        ],
    );
    a.successors.clear();
    append(
        &mut graph,
        &[
            declaration(&a, Some(Existing(ids[0]))),
            node("B", v(1), Some(Existing(ids[2]))),
            node("A", v(1), None),
            node("B", v(2), Some(Existing(ids[3]))),
        ],
    );
    graphs.push(graph);

    // Missing parent types: a declared successor of an undeclared root, and a
    // branch from a stale node whose declaration was retired. When a child keeps
    // its parent's missing type, the repeated judgment equals the current-state
    // finding and deduplicates; the declared successor's does not.
    let mut graph = LifeCoreList::default();
    let ids = append(
        &mut graph,
        &[
            declaration(&with_field("A"), None),
            node("X", Body::new(), None),
            node("A", Body::new(), Some(Added(1))),
            node("A", v(1), None),
            node("A", v(2), Some(Added(3))),
        ],
    );
    append(
        &mut graph,
        &[
            declaration(&with_field("B"), Some(Existing(ids[0]))),
            node("A", v(3), Some(Existing(ids[3]))),
            node("X", v(1), Some(Existing(ids[1]))),
        ],
    );
    graphs.push(graph);

    // An ambiguous parent type, later disambiguated by renaming a declaration.
    let mut graph = LifeCoreList::default();
    let mut other_a = with_field("A");
    other_a.outgoing.insert("link".into());
    let ids = append(
        &mut graph,
        &[
            declaration(&with_field("A"), None),
            declaration(&other_a, None),
            node("A", Body::new(), None),
            node("A", v(1), Some(Added(2))),
        ],
    );
    append(
        &mut graph,
        &[
            declaration(&with_field("C"), Some(Existing(ids[1]))),
            node("A", v(2), Some(Existing(ids[3]))),
        ],
    );
    graphs.push(graph);

    // A malformed declaration blocks resolving the parent type until repaired.
    let mut graph = LifeCoreList::default();
    let ids = append(
        &mut graph,
        &[
            declaration(&with_field("A"), None),
            node("A", Body::new(), None),
            node("Type", Body::new(), None),
            node("A", v(1), Some(Added(1))),
        ],
    );
    append(
        &mut graph,
        &[
            declaration(&with_field("B"), Some(Existing(ids[2]))),
            node("A", v(2), Some(Existing(ids[3]))),
        ],
    );
    graphs.push(graph);

    // Edges whose endpoints are superseded, a permitted change into a
    // declaration, and a forbidden change between ordinary types, followed by
    // an unrelated append.
    let mut graph = LifeCoreList::default();
    let a = Type::new("A", &[], &["link"], &[], &["Type"]);
    let b = Type::new("B", &[], &[], &["link"], &[]);
    let ids = append(
        &mut graph,
        &[
            declaration(&a, None),
            declaration(&b, None),
            node("A", Body::new(), None),
            node("B", Body::new(), None),
            Change::Entry(EntryChange::Edge(NewEdge {
                from: Added(2),
                ty_name: "link".into(),
                to: Added(3),
            })),
        ],
    );
    append(
        &mut graph,
        &[
            declaration(&with_field("C"), Some(Existing(ids[2]))),
            node("B", Body::new(), Some(Existing(ids[3]))),
            node("A", Body::from([("x".into(), true.into())]), None),
            Change::Entry(EntryChange::Edge(NewEdge {
                from: Added(2),
                ty_name: "link".into(),
                to: Added(1),
            })),
            node("B", Body::new(), Some(Added(2))),
            declaration(&with_field("D"), None),
        ],
    );
    graphs.push(graph);

    // A declaration revised into an ordinary node, immediately followed by an
    // edge from that node, so the prefix ending at the edge must not judge it.
    let mut graph = LifeCoreList::default();
    let linked = Type::new("A", &[], &["link"], &["link"], &[]);
    append(
        &mut graph,
        &[
            declaration(&linked, None),
            declaration(&with_field("B"), None),
            node("A", Body::new(), None),
            node("A", Body::new(), Some(Added(1))),
            Change::Entry(EntryChange::Edge(NewEdge {
                from: Added(3),
                ty_name: "link".into(),
                to: Added(2),
            })),
            declaration(&with_field("C"), None),
        ],
    );
    graphs.push(graph);

    // Each fixture has later prefixes at which the former loop repeated a
    // transition judgment.
    for graph in &graphs {
        assert!(!graph.view().is_empty());
        assert!(assert_replay_matches_all_children(graph) > 0);
    }
}

#[test]
fn messages_name_the_field_path_and_the_issue_in_words() {
    use FieldIssueKind::*;
    use PathPart::{Field as F, Index};
    let mut graph = LifeCoreList::default();
    let subject = append(&mut graph, &[node("A", Body::new(), None)])[0];
    let message = |finding: TypeCheckMessage| {
        TypeCheck::default()
            .convert_message(finding)
            .diagnostic
            .message
    };
    let field = |path: Vec<PathPart>, kind| {
        message(TypeCheckMessage::Field {
            subject,
            issue: lifecore::typing::FieldIssue { path, kind },
        })
    };
    let email = || vec![F("email".into())];
    assert_eq!(
        field(email(), MissingMandatory),
        "mandatory field email is missing"
    );
    assert_eq!(
        field(email(), MissingEncouraged),
        "encouraged field email is missing"
    );
    assert_eq!(
        field(email(), Deprecated),
        "deprecated field email is present"
    );
    assert_eq!(
        field(email(), UnexpectedField),
        "field email is not declared"
    );
    assert_eq!(
        field(
            vec![F("priority".into())],
            TypeMismatch {
                expected: PayloadKind::Integer,
                actual: PayloadKind::Text,
            }
        ),
        "field priority should be integer, found text"
    );
    assert_eq!(
        field(
            vec![F("details".into()), F("owner".into())],
            MissingMandatory
        ),
        "mandatory field details.owner is missing"
    );
    assert_eq!(
        field(
            vec![F("tags".into()), Index(1)],
            TypeMismatch {
                expected: PayloadKind::Text,
                actual: PayloadKind::Integer,
            }
        ),
        "field tags[1] should be text, found integer"
    );
    assert_eq!(
        message(TypeCheckMessage::MalformedDeclaration {
            subject,
            error: SchemaError {
                path: vec![F("fields".into()), F("title".into()), Index(1)],
                kind: SchemaErrorKind::UnknownShape("txt".into()),
            },
        }),
        r#"invalid schema at fields.title[1]: unknown shape word "txt""#
    );
    assert_eq!(
        message(TypeCheckMessage::BadEdgeEndpoint {
            subject,
            edge_type: "PartOf".into(),
            node_type: "Task".into(),
            outgoing: true,
        }),
        r#"type "Task" does not permit outgoing edge "PartOf""#
    );
    assert_eq!(
        message(TypeCheckMessage::ReservedTypeName { subject }),
        r#"the name "Type" is reserved for the built-in metatype"#
    );
}
