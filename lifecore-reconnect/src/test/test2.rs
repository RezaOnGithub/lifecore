use crate::{
    LostConnection, LostEdge, Reconnect, ReconnectConfig, ReconnectConfigError, ReconnectFactory,
    SubgraphFilter, lost_connections,
};
use lifecore::{
    BasicValidationFailure, Body, EntryId, LifeCoreError, LifeCoreList,
    analysis::{
        AnalysisOutput, AnalysisRunner, Analyzer, AnalyzerFactory, Diagnostic, EvaluatedView,
        FindingKey, MessageKind, Observation,
        presentation::{DismissalReason, Dismissals, ReportOptions},
        suggestion::{self, prepare_suggestion},
    },
    transaction::{
        self, Change, Checked, EntryChange, NewEdge, NewNode,
        Reference::{self, Added, Existing},
        RevisionIntent, prepare,
    },
    typing::{FieldStatus, Type, ValueType},
};
use lifecore_typecheck::{TypeCheckFactory, TypeCheckMessage};
use std::collections::BTreeSet;

fn runner() -> AnalysisRunner {
    let mut runner = AnalysisRunner::default();
    runner
        .register_analyzer_kind(Box::new(ReconnectFactory))
        .unwrap();
    runner
        .register_analyzer_kind(Box::new(TypeCheckFactory))
        .unwrap();
    runner
}
fn configure(name: &str, configuration: &str) -> Change {
    Change::AddAnalyzer {
        name: name.into(),
        configuration: configuration.into(),
    }
}
// An empty database whose suite runs Reconnect alone, unconfigured.
fn database() -> LifeCoreList {
    let mut graph = LifeCoreList::default();
    append(&mut graph, &[configure(ReconnectFactory::name(), "")]);
    graph
}
fn append(graph: &mut LifeCoreList, changes: &[Change]) -> Vec<EntryId> {
    let plan = prepare(graph, changes, &mut runner())
        .unwrap()
        .accepted()
        .unwrap();
    let ids = plan.added();
    plan.commit(graph).unwrap();
    ids
}
fn append_one(graph: &mut LifeCoreList, change: Change) -> EntryId {
    let [id] = append(graph, &[change]).try_into().unwrap();
    id
}
fn node(ty_name: &str, title: &str, predecessor: Option<(Reference, RevisionIntent)>) -> Change {
    Change::Entry(EntryChange::Node(NewNode {
        ty_name: ty_name.into(),
        payload: Body::from([("title".into(), title.into())]),
        predecessor: predecessor.map(|(reference, _)| reference),
        intent: predecessor.map(|(_, intent)| intent),
    }))
}
fn root(ty_name: &str, title: &str) -> Change {
    node(ty_name, title, None)
}
fn revise(ty_name: &str, title: &str, predecessor: Reference) -> Change {
    node(ty_name, title, Some((predecessor, RevisionIntent::Revise)))
}
fn branch(ty_name: &str, title: &str, predecessor: Reference) -> Change {
    node(ty_name, title, Some((predecessor, RevisionIntent::Branch)))
}
fn new_edge(ty_name: &str, from: Reference, to: Reference) -> EntryChange {
    EntryChange::Edge(NewEdge {
        ty_name: ty_name.into(),
        from,
        to,
    })
}
fn edge(ty_name: &str, from: Reference, to: Reference) -> Change {
    Change::Entry(new_edge(ty_name, from, to))
}
fn link(ty_name: &str, from: EntryId, to: EntryId) -> Change {
    edge(ty_name, Existing(from), Existing(to))
}
fn lost(
    edge: EntryId,
    ty_name: &str,
    predecessor: EntryId,
    successor: EntryId,
    other: EntryId,
    outgoing: bool,
) -> LostConnection {
    LostConnection {
        edge,
        ty_name: ty_name.into(),
        predecessor,
        successor,
        other,
        outgoing,
    }
}
fn config(json: &str) -> ReconnectConfig {
    ReconnectConfig::from_json(json).unwrap()
}
// The typed findings at the prefix whose newest entry is `newest`.
fn judged(graph: &LifeCoreList, newest: EntryId) -> Vec<LostConnection> {
    judged_with(graph, newest, &ReconnectConfig::default())
}
fn judged_with(
    graph: &LifeCoreList,
    newest: EntryId,
    config: &ReconnectConfig,
) -> Vec<LostConnection> {
    lost_connections(graph.view().prefix(newest.position() + 1).unwrap(), config)
}
fn analysis(graph: &LifeCoreList) -> AnalysisOutput {
    runner().run(graph).unwrap()
}
fn certificate(edge: EntryId, successor: EntryId) -> String {
    serde_json::to_string(&LostEdge { edge, successor }).unwrap()
}
fn observation(output: &AnalysisOutput, edge: EntryId, successor: EntryId) -> &Observation {
    let certificate = certificate(edge, successor);
    output
        .suggestions()
        .find(|observation| observation.key.analyzer_certificate.0 == certificate)
        .expect("suggestion for the lost edge and successor")
}
fn visible(output: &AnalysisOutput, dismissals: &Dismissals, key: &FindingKey) -> bool {
    output
        .groups(dismissals, ReportOptions::default())
        .iter()
        .any(|group| group.key == key)
}

#[test]
fn a_revision_is_offered_each_edge_of_its_predecessor_at_its_own_prefix_only() {
    let mut graph = database();
    let [d, out, inc, e_out, e_in] = append(
        &mut graph,
        &[
            root("Note", "d"),
            root("Note", "cited"),
            root("Note", "citing"),
            edge("Cites", Added(0), Added(1)),
            edge("Cites", Added(2), Added(0)),
        ],
    )
    .try_into()
    .unwrap();
    let mut analyzer = Reconnect::new(ReconnectConfig::default());
    assert!(analyzer.initialize(&Default::default()).unwrap().is_empty());
    for end_exclusive in 0..=graph.view().len() {
        let prefix = graph.view().prefix(end_exclusive).unwrap();
        assert!(lost_connections(prefix, &ReconnectConfig::default()).is_empty());
    }

    let s = append_one(&mut graph, revise("Note", "s", Existing(d)));
    let findings = judged(&graph, s);
    assert_eq!(
        findings,
        vec![
            lost(e_out, "Cites", d, s, out, true),
            lost(e_in, "Cites", d, s, inc, false),
        ]
    );
    assert_eq!(
        findings
            .iter()
            .map(LostConnection::action)
            .collect::<Vec<_>>(),
        vec![
            new_edge("Cites", Existing(s), Existing(out)),
            new_edge("Cites", Existing(inc), Existing(s)),
        ]
    );

    let [later, later_edge] = append(
        &mut graph,
        &[root("Note", "later"), edge("Cites", Existing(s), Added(0))],
    )
    .try_into()
    .unwrap();
    assert!(judged(&graph, later).is_empty());
    assert!(judged(&graph, later_edge).is_empty());

    let output = analysis(&graph);
    let at_s = graph.view().prefix(s.position() + 1).unwrap().id();
    let observations = output.suggestions().collect::<Vec<_>>();
    assert_eq!(observations.len(), 2);
    for (observation, (lost_edge, other, action)) in observations.into_iter().zip([
        (e_out, out, new_edge("Cites", Existing(s), Existing(out))),
        (e_in, inc, new_edge("Cites", Existing(inc), Existing(s))),
    ]) {
        assert_eq!(observation.key.analyzer.name, ReconnectFactory::name());
        assert_eq!(
            observation.key.analyzer_certificate.0,
            certificate(lost_edge, s)
        );
        assert_eq!(
            observation.evaluated_view,
            EvaluatedView::Prefixes {
                first: at_s,
                last: at_s
            }
        );
        assert_eq!(
            observation.diagnostic,
            Diagnostic {
                subjects: BTreeSet::from([lost_edge, d, s, other]),
                focus: Some(s),
                message: observation.diagnostic.message.clone(),
                kind: MessageKind::Suggestion(vec![action]),
                sticky: true,
            }
        );
    }
    // The text names the label and direction and no boundary.
    assert_eq!(
        output
            .suggestions()
            .map(|observation| observation.diagnostic.message.as_str())
            .collect::<Vec<_>>(),
        [
            r#"revision does not carry its predecessor's outgoing edge "Cites""#,
            r#"revision does not carry its predecessor's incoming edge "Cites""#,
        ]
    );
}

#[test]
fn the_certificate_is_the_json_encoding_of_the_lost_edge_and_successor() {
    let mut graph = database();
    let [d, _, e] = append(
        &mut graph,
        &[
            root("Note", "d"),
            root("Note", "o"),
            edge("Cites", Added(0), Added(1)),
        ],
    )
    .try_into()
    .unwrap();
    let s = append_one(&mut graph, revise("Note", "s", Existing(d)));
    let output = analysis(&graph);
    assert_eq!(
        observation(&output, e, s).key.analyzer_certificate.0,
        format!(
            r#"{{"edge":{{"position":"{}","origin":"{}"}},"successor":{{"position":"{}","origin":"{}"}}}}"#,
            e.position(),
            e.origin(),
            s.position(),
            s.origin()
        )
    );
}

#[test]
fn a_consecutive_split_offers_both_branches_the_same_edges() {
    let mut graph = database();
    let [d, o1, o2, e1, e2] = append(
        &mut graph,
        &[
            root("Note", "d"),
            root("Note", "o1"),
            root("Note", "o2"),
            edge("Cites", Added(0), Added(1)),
            edge("Cites", Added(2), Added(0)),
        ],
    )
    .try_into()
    .unwrap();
    let s1 = append_one(&mut graph, revise("Note", "s1", Existing(d)));
    append_one(&mut graph, root("Note", "unrelated"));
    let s2 = append_one(&mut graph, branch("Note", "s2", Existing(d)));
    assert_eq!(
        judged(&graph, s1),
        vec![
            lost(e1, "Cites", d, s1, o1, true),
            lost(e2, "Cites", d, s1, o2, false),
        ]
    );
    // `d` is already superseded at `s2`'s prefix and is judged all the same.
    assert_eq!(
        graph.view().prefix(s2.position()).unwrap().is_stale(d),
        Some(true)
    );
    assert_eq!(
        judged(&graph, s2),
        vec![
            lost(e1, "Cites", d, s2, o1, true),
            lost(e2, "Cites", d, s2, o2, false),
        ]
    );

    let before = analysis(&graph);
    let (first, second) = (observation(&before, e1, s1), observation(&before, e1, s2));
    assert_eq!(
        FindingKey {
            analyzer_certificate: first.key.analyzer_certificate.clone(),
            ..second.key.clone()
        },
        first.key
    );
    assert_ne!(first.key, second.key);

    let for_s2 = |output: &AnalysisOutput| {
        output
            .suggestions()
            .filter(|observation| observation.diagnostic.focus == Some(s2))
            .cloned()
            .collect::<Vec<_>>()
    };
    let unchanged = for_s2(&before);
    assert_eq!(unchanged.len(), 2);
    append(&mut graph, &[link("Cites", s1, o1), link("Cites", o2, s1)]);
    assert_eq!(for_s2(&analysis(&graph)), unchanged);
}

#[test]
fn a_chain_copies_only_what_its_own_predecessor_carries() {
    let mut graph = database();
    let [d, o, e] = append(
        &mut graph,
        &[
            root("Note", "d"),
            root("Note", "o"),
            edge("Cites", Added(0), Added(1)),
        ],
    )
    .try_into()
    .unwrap();
    let s1 = append_one(&mut graph, revise("Note", "s1", Existing(d)));
    let mut unreconnected = graph.clone();

    let e1 = append_one(&mut graph, link("Cites", s1, o));
    let s2 = append_one(&mut graph, revise("Note", "s2", Existing(s1)));
    assert_eq!(judged(&graph, s2), vec![lost(e1, "Cites", s1, s2, o, true)]);
    assert_eq!(
        observation(&analysis(&graph), e1, s2)
            .key
            .analyzer_certificate
            .0,
        certificate(e1, s2)
    );

    let s2 = append_one(&mut unreconnected, revise("Note", "s2", Existing(s1)));
    assert!(judged(&unreconnected, s2).is_empty());
    let output = analysis(&unreconnected);
    let record = observation(&output, e, s1);
    assert!(visible(&output, &Dismissals::default(), &record.key));
}

#[test]
fn a_superseded_other_endpoint_suppresses_the_suggestion() {
    let mut graph = database();
    let [d, o, e] = append(
        &mut graph,
        &[
            root("Note", "d"),
            root("Note", "o"),
            edge("Cites", Added(0), Added(1)),
        ],
    )
    .try_into()
    .unwrap();
    let mut successor_first = graph.clone();

    append_one(&mut graph, revise("Note", "o2", Existing(o)));
    let s = append_one(&mut graph, revise("Note", "s", Existing(d)));
    assert!(judged(&graph, s).is_empty());

    let s = append_one(&mut successor_first, revise("Note", "s", Existing(d)));
    let expected = vec![lost(e, "Cites", d, s, o, true)];
    assert_eq!(judged(&successor_first, s), expected);
    let o2 = append_one(&mut successor_first, revise("Note", "o2", Existing(o)));
    assert!(judged(&successor_first, o2).is_empty());
    assert_eq!(judged(&successor_first, s), expected);
}

#[test]
fn revising_both_endpoints_in_one_plan_names_the_old_endpoint() {
    let mut graph = database();
    let [d, o, e] = append(
        &mut graph,
        &[
            root("Note", "d"),
            root("Note", "o"),
            edge("Cites", Added(0), Added(1)),
        ],
    )
    .try_into()
    .unwrap();
    let [s, o2, repair] = append(
        &mut graph,
        &[
            revise("Note", "s", Existing(d)),
            revise("Note", "o2", Existing(o)),
            edge("Cites", Added(0), Added(1)),
        ],
    )
    .try_into()
    .unwrap();
    let findings = judged(&graph, s);
    assert_eq!(findings, vec![lost(e, "Cites", d, s, o, true)]);
    assert_eq!(
        findings[0].action(),
        new_edge("Cites", Existing(s), Existing(o))
    );
    assert!(judged(&graph, o2).is_empty());
    assert!(judged(&graph, repair).is_empty());

    let output = analysis(&graph);
    assert!(matches!(
        prepare_suggestion(&graph, observation(&output, e, s), &mut runner()),
        Err(suggestion::Error::Check(transaction::Error::Core(
            LifeCoreError::BasicValidationFailure(BasicValidationFailure::StaleEndpoint { .. })
        )))
    ));
}

#[test]
fn an_edge_recreated_by_hand_leaves_the_sticky_suggestion_until_dismissed() {
    let mut graph = database();
    let [d, o, e] = append(
        &mut graph,
        &[
            root("Note", "d"),
            root("Note", "o"),
            edge("Cites", Added(0), Added(1)),
        ],
    )
    .try_into()
    .unwrap();
    let s = append_one(&mut graph, revise("Note", "s", Existing(d)));
    append_one(&mut graph, link("Cites", s, o));

    let output = analysis(&graph);
    let record = observation(&output, e, s);
    let at_s = graph.view().prefix(s.position() + 1).unwrap().id();
    assert_eq!(
        record.evaluated_view,
        EvaluatedView::Prefixes {
            first: at_s,
            last: at_s
        }
    );
    let mut dismissals = Dismissals::default();
    assert!(visible(&output, &dismissals, &record.key));
    assert!(matches!(
        prepare_suggestion(&graph, record, &mut runner()),
        Err(suggestion::Error::Check(transaction::Error::Core(
            LifeCoreError::BasicValidationFailure(BasicValidationFailure::Duplicate { .. })
        )))
    ));
    dismissals
        .dismiss(record.key.clone(), DismissalReason::NoLongerRelevant, None)
        .unwrap();
    assert!(!visible(&output, &dismissals, &record.key));
}

#[test]
fn losing_a_reasserted_connection_again_has_a_distinct_key() {
    let mut graph = database();
    let [d, o, e] = append(
        &mut graph,
        &[
            root("Note", "d"),
            root("Note", "o"),
            edge("Cites", Added(0), Added(1)),
        ],
    )
    .try_into()
    .unwrap();
    let s = append_one(&mut graph, revise("Note", "s", Existing(d)));
    let e2 = append_one(&mut graph, link("Cites", s, o));
    let s_next = append_one(&mut graph, revise("Note", "s'", Existing(s)));
    assert_eq!(
        judged(&graph, s_next),
        vec![lost(e2, "Cites", s, s_next, o, true)]
    );

    let output = analysis(&graph);
    let first = observation(&output, e, s).key.clone();
    let second = observation(&output, e2, s_next).key.clone();
    assert_ne!(first, second);
    let mut dismissals = Dismissals::default();
    dismissals
        .dismiss(first.clone(), DismissalReason::Acknowledged, None)
        .unwrap();
    assert!(!visible(&output, &dismissals, &first));
    assert!(visible(&output, &dismissals, &second));
    assert_eq!(analysis(&graph), output);
}

#[test]
fn a_successor_type_that_forbids_the_label_rejects_the_suggestion() {
    let title = [("title", (ValueType::Text, FieldStatus::Mandatory))];
    let declare = |ty: Type| {
        Change::Entry(EntryChange::Node(NewNode {
            ty_name: Type::TYPE_NAME.into(),
            payload: ty.to_body(),
            predecessor: None,
            intent: None,
        }))
    };
    let mut graph = LifeCoreList::default();
    append(
        &mut graph,
        &[
            configure(TypeCheckFactory::name(), ""),
            configure(ReconnectFactory::name(), ""),
            declare(Type::new("Linked", &title, &["L"], &[], &["Plain"])),
            declare(Type::new("Plain", &title, &[], &[], &[])),
            declare(Type::new("Target", &title, &[], &["L"], &[])),
        ],
    );
    let [d, o, e] = append(
        &mut graph,
        &[
            root("Linked", "d"),
            root("Target", "o"),
            edge("L", Added(0), Added(1)),
        ],
    )
    .try_into()
    .unwrap();
    let s = append_one(&mut graph, revise("Plain", "s", Existing(d)));
    assert_eq!(judged(&graph, s), vec![lost(e, "L", d, s, o, true)]);

    let output = analysis(&graph);
    let Checked::Rejected(rejection) =
        prepare_suggestion(&graph, observation(&output, e, s), &mut runner()).unwrap()
    else {
        panic!("TypeCheck must reject the equivalent edge");
    };
    let added = rejection.operations()[0].unwrap();
    let expected = serde_json::to_string(&TypeCheckMessage::BadEdgeEndpoint {
        subject: added,
        edge_type: "L".into(),
        node_type: "Plain".into(),
        outgoing: true,
    })
    .unwrap();
    assert!(rejection.analysis().errors().any(|error| {
        error.key.analyzer.name == TypeCheckFactory::name()
            && error.key.analyzer_certificate.0 == expected
    }));
}

#[test]
fn configuration_filters_labels_and_successor_types_and_is_strict() {
    assert_eq!(config(""), ReconnectConfig::default());
    assert_eq!(config("{}"), ReconnectConfig::default());
    assert_eq!(
        config(r#"{"filter":{"edge_types":["B","A","A"]}}"#),
        ReconnectConfig {
            filter: Some(SubgraphFilter {
                node_types: None,
                edge_types: Some(BTreeSet::from(["A".into(), "B".into()])),
            })
        }
    );
    for invalid in [
        r#"{"filters":{}}"#,
        r#"{"filter":{"labels":[]}}"#,
        r#"{"filter":{},"filter":{}}"#,
        r#"{"filter":{"edge_types":[1]}}"#,
        " ",
        "null",
    ] {
        assert!(
            matches!(
                ReconnectConfig::from_json(invalid),
                Err(ReconnectConfigError::Json(_))
            ),
            "{invalid:?}"
        );
    }
    // Serde would read these as an empty or partial configuration.
    for (invalid, field) in [
        ("[]", "the configuration"),
        ("[null]", "the configuration"),
        (r#"{"filter":null}"#, "`filter`"),
        (r#"{"filter":[["Note"]]}"#, "`filter`"),
        (r#"{"filter":{"node_types":null}}"#, "`node_types`"),
        (r#"{"filter":{"edge_types":null}}"#, "`edge_types`"),
    ] {
        assert!(
            matches!(
                ReconnectConfig::from_json(invalid),
                Err(ReconnectConfigError::Shape(name, _)) if name == field
            ),
            "{invalid:?}"
        );
    }
    assert_eq!(
        config(r#"{"filter":{}}"#),
        ReconnectConfig {
            filter: Some(SubgraphFilter::default())
        }
    );
    assert!(matches!(
        ReconnectFactory.analyzer_with_config(r#"{"filters":{}}"#),
        Err(LifeCoreError::InvalidAnalyzerConfiguration { analyzer_name, .. })
            if analyzer_name == ReconnectFactory::name()
    ));

    let mut graph = database();
    let [d, t, o1, o2, cites, about, topic_edge] = append(
        &mut graph,
        &[
            root("Note", "d"),
            root("Topic", "t"),
            root("Note", "o1"),
            root("Note", "o2"),
            edge("Cites", Added(0), Added(2)),
            edge("About", Added(0), Added(3)),
            edge("About", Added(2), Added(1)),
        ],
    )
    .try_into()
    .unwrap();
    let s = append_one(&mut graph, revise("Note", "s", Existing(d)));
    let t2 = append_one(&mut graph, revise("Topic", "t2", Existing(t)));
    let by_label = config(r#"{"filter":{"edge_types":["Cites"]}}"#);
    assert_eq!(
        judged_with(&graph, s, &by_label),
        vec![lost(cites, "Cites", d, s, o1, true)]
    );
    assert!(judged_with(&graph, t2, &by_label).is_empty());
    let by_type = config(r#"{"filter":{"node_types":["Note"]}}"#);
    assert_eq!(
        judged_with(&graph, s, &by_type),
        vec![
            lost(cites, "Cites", d, s, o1, true),
            lost(about, "About", d, s, o2, true),
        ]
    );
    assert!(judged_with(&graph, t2, &by_type).is_empty());
    assert_eq!(
        judged(&graph, t2),
        vec![lost(topic_edge, "About", t, t2, o1, false)]
    );

    // `""` and `"{}"` find the same lost edges under distinct identities.
    append(&mut graph, &[configure(ReconnectFactory::name(), "{}")]);
    let output = analysis(&graph);
    let under = |configuration: &str| {
        output
            .suggestions()
            .filter(|observation| observation.key.analyzer.configuration == configuration)
            .map(|observation| {
                (
                    observation.key.analyzer_certificate.clone(),
                    observation.evaluated_view,
                    observation.diagnostic.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(under("").len(), 3);
    assert_eq!(under(""), under("{}"));
}

#[test]
fn later_appends_never_change_earlier_observations() {
    let mut graph = database();
    let [d1, d2, o, e1, e2] = append(
        &mut graph,
        &[
            root("Note", "d1"),
            root("Note", "d2"),
            root("Note", "o"),
            edge("Cites", Added(0), Added(2)),
            edge("About", Added(2), Added(1)),
        ],
    )
    .try_into()
    .unwrap();
    // Two supersessions in one plan are judged at their own prefixes.
    let [s1, s2] = append(
        &mut graph,
        &[
            revise("Note", "s1", Existing(d1)),
            revise("Note", "s2", Existing(d2)),
        ],
    )
    .try_into()
    .unwrap();
    assert_eq!(judged(&graph, s1), vec![lost(e1, "Cites", d1, s1, o, true)]);
    assert_eq!(
        judged(&graph, s2),
        vec![lost(e2, "About", d2, s2, o, false)]
    );

    let earlier = analysis(&graph);
    assert_eq!(earlier.suggestions().count(), 2);
    let [_, _, s1_next] = append(
        &mut graph,
        &[
            link("Cites", s1, o),
            root("Note", "n"),
            revise("Note", "s1'", Existing(s1)),
        ],
    )
    .try_into()
    .unwrap();
    append(&mut graph, &[revise("Note", "o'", Existing(o))]);
    let later = analysis(&graph);
    assert!(
        earlier
            .observations()
            .all(|observation| later.observations().any(|other| other == observation))
    );
    assert_eq!(later.suggestions().count(), 3);
    assert_eq!(judged(&graph, s1_next).len(), 1);
}

#[test]
fn a_late_branch_is_offered_only_the_edges_no_sibling_carries() {
    let mut base = database();
    let [d, o1, o2, e1, e2] = append(
        &mut base,
        &[
            root("Note", "d"),
            root("Note", "o1"),
            root("Note", "o2"),
            edge("Cites", Added(0), Added(1)),
            edge("Cites", Added(0), Added(2)),
        ],
    )
    .try_into()
    .unwrap();
    let s1 = append_one(&mut base, revise("Note", "s1", Existing(d)));

    let mut graph = base.clone();
    append_one(&mut graph, link("Cites", s1, o1));
    let s2 = append_one(&mut graph, branch("Note", "s2", Existing(d)));
    assert_eq!(judged(&graph, s2), vec![lost(e2, "Cites", d, s2, o2, true)]);
    assert_eq!(
        observation(&analysis(&graph), e2, s2)
            .key
            .analyzer_certificate
            .0,
        certificate(e2, s2)
    );

    let mut early = base.clone();
    let s2 = append_one(&mut early, branch("Note", "s2", Existing(d)));
    let both = vec![
        lost(e1, "Cites", d, s2, o1, true),
        lost(e2, "Cites", d, s2, o2, true),
    ];
    assert_eq!(judged(&early, s2), both);
    append_one(&mut early, link("Cites", s1, o1));
    assert_eq!(judged(&early, s2), both);

    let mut everything = base;
    append(
        &mut everything,
        &[link("Cites", s1, o1), link("Cites", s1, o2)],
    );
    let s2 = append_one(&mut everything, branch("Note", "s2", Existing(d)));
    assert!(judged(&everything, s2).is_empty());
}

#[test]
fn only_an_exact_equivalent_edge_claims() {
    let mut base = database();
    let [d, o, e1] = append(
        &mut base,
        &[
            root("Note", "d"),
            root("Note", "o"),
            edge("Cites", Added(0), Added(1)),
        ],
    )
    .try_into()
    .unwrap();
    let s1 = append_one(&mut base, revise("Note", "s1", Existing(d)));

    for near_miss in [link("About", s1, o), link("Cites", o, s1)] {
        let mut graph = base.clone();
        append_one(&mut graph, near_miss);
        let s2 = append_one(&mut graph, branch("Note", "s2", Existing(d)));
        assert_eq!(judged(&graph, s2), vec![lost(e1, "Cites", d, s2, o, true)]);
    }

    // A revision of `o` supersedes it, so `e1` is suppressed for its other
    // endpoint whether or not a sibling's edge to the revision could claim it.
    let mut graph = base.clone();
    let o2 = append_one(&mut graph, revise("Note", "o2", Existing(o)));
    append_one(&mut graph, link("Cites", s1, o2));
    let s2 = append_one(&mut graph, branch("Note", "s2", Existing(d)));
    assert!(judged(&graph, s2).is_empty());

    // An edge the sibling was created with claims like a reconnected one.
    let mut graph = database();
    let [d, o, _] = append(
        &mut graph,
        &[
            root("Note", "d"),
            root("Note", "o"),
            edge("Cites", Added(0), Added(1)),
        ],
    )
    .try_into()
    .unwrap();
    let [s1, _] = append(
        &mut graph,
        &[
            revise("Note", "s1", Existing(d)),
            edge("Cites", Added(0), Existing(o)),
        ],
    )
    .try_into()
    .unwrap();
    assert!(!judged(&graph, s1).is_empty());
    let s2 = append_one(&mut graph, branch("Note", "s2", Existing(d)));
    assert!(judged(&graph, s2).is_empty());
}

// The mirror of the late-branch and exact-claim tests for an edge that ends
// at the predecessor: only a sibling's edge from `o` to it claims.
#[test]
fn an_incoming_edge_is_claimed_only_by_an_incoming_equivalent() {
    let mut base = database();
    let [d, o, e] = append(
        &mut base,
        &[
            root("Note", "d"),
            root("Note", "o"),
            edge("Cites", Added(1), Added(0)),
        ],
    )
    .try_into()
    .unwrap();
    let s1 = append_one(&mut base, revise("Note", "s1", Existing(d)));
    assert_eq!(judged(&base, s1), vec![lost(e, "Cites", d, s1, o, false)]);

    let mut graph = base.clone();
    append_one(&mut graph, link("Cites", o, s1));
    let s2 = append_one(&mut graph, branch("Note", "s2", Existing(d)));
    assert!(judged(&graph, s2).is_empty());

    for near_miss in [link("About", o, s1), link("Cites", s1, o)] {
        let mut graph = base.clone();
        append_one(&mut graph, near_miss);
        let s2 = append_one(&mut graph, branch("Note", "s2", Existing(d)));
        assert_eq!(judged(&graph, s2), vec![lost(e, "Cites", d, s2, o, false)]);
    }

    // A reconnection appended after the branch never reaches its prefix.
    let mut graph = base;
    let s2 = append_one(&mut graph, branch("Note", "s2", Existing(d)));
    let offered = vec![lost(e, "Cites", d, s2, o, false)];
    assert_eq!(judged(&graph, s2), offered);
    append_one(&mut graph, link("Cites", o, s1));
    assert_eq!(judged(&graph, s2), offered);
}

#[test]
fn only_current_siblings_claim_through_a_chain() {
    let mut graph = database();
    let [d, o1, o2, e1, e2] = append(
        &mut graph,
        &[
            root("Note", "d"),
            root("Note", "o1"),
            root("Note", "o2"),
            edge("Cites", Added(0), Added(1)),
            edge("Cites", Added(0), Added(2)),
        ],
    )
    .try_into()
    .unwrap();
    let s1 = append_one(&mut graph, revise("Note", "s1", Existing(d)));
    let [_, e2_on_s1] = append(&mut graph, &[link("Cites", s1, o1), link("Cites", s1, o2)])
        .try_into()
        .unwrap();
    let s1_next = append_one(&mut graph, revise("Note", "s1'", Existing(s1)));
    append_one(&mut graph, link("Cites", s1_next, o1));
    let s2 = append_one(&mut graph, branch("Note", "s2", Existing(d)));
    assert_eq!(judged(&graph, s2), vec![lost(e2, "Cites", d, s2, o2, true)]);
    assert!(!judged(&graph, s2).iter().any(|finding| finding.edge == e1));

    let output = analysis(&graph);
    let record = observation(&output, e2_on_s1, s1_next);
    assert!(visible(&output, &Dismissals::default(), &record.key));
}

#[test]
fn the_filter_never_restricts_which_siblings_claim() {
    let mut base = database();
    let [d, o1, o2, e1, e2] = append(
        &mut base,
        &[
            root("Note", "d"),
            root("Note", "o1"),
            root("Note", "o2"),
            edge("Cites", Added(0), Added(1)),
            edge("About", Added(0), Added(2)),
        ],
    )
    .try_into()
    .unwrap();
    let s1 = append_one(&mut base, revise("Draft", "s1", Existing(d)));

    let mut claimed = base.clone();
    append_one(&mut claimed, link("Cites", s1, o1));
    let s2 = append_one(&mut claimed, branch("Note", "s2", Existing(d)));
    let mut unclaimed = base;
    let s2_unclaimed = append_one(&mut unclaimed, branch("Note", "s2", Existing(d)));

    let notes = config(r#"{"filter":{"node_types":["Note"]}}"#);
    assert!(judged_with(&claimed, s1, &notes).is_empty());
    assert_eq!(
        judged_with(&claimed, s2, &notes),
        vec![lost(e2, "About", d, s2, o2, true)]
    );

    let about = config(r#"{"filter":{"edge_types":["About"]}}"#);
    assert_eq!(
        judged_with(&claimed, s2, &about),
        vec![lost(e2, "About", d, s2, o2, true)]
    );
    assert_eq!(
        judged_with(&unclaimed, s2_unclaimed, &about),
        vec![lost(e2, "About", d, s2_unclaimed, o2, true)]
    );
    assert_eq!(
        judged(&unclaimed, s2_unclaimed),
        vec![
            lost(e1, "Cites", d, s2_unclaimed, o1, true),
            lost(e2, "About", d, s2_unclaimed, o2, true),
        ]
    );
}

// RECONNECT-EXPLAINER's database is addressed by position.
fn id_at(graph: &LifeCoreList, position: usize) -> EntryId {
    graph.view().at(position).unwrap().id()
}
fn existing_at(graph: &LifeCoreList, position: usize) -> Reference {
    Existing(id_at(graph, position))
}
// Each suggestion at the prefix ending with position `newest`, as the
// positions of the lost edge and of the suggested edge's `from` and `to`.
fn suggested(graph: &LifeCoreList, newest: usize) -> Vec<(usize, usize, usize)> {
    judged(graph, id_at(graph, newest))
        .iter()
        .map(|finding| {
            let EntryChange::Edge(NewEdge {
                from: Existing(from),
                to: Existing(to),
                ..
            }) = finding.action()
            else {
                panic!("an edge between existing entries");
            };
            (finding.edge.position(), from.position(), to.position())
        })
        .collect()
}
// Applies a suggestion as the explainer applies its export, returning the
// position of the added edge.
fn accept(graph: &mut LifeCoreList, edge: usize, successor: usize) -> usize {
    let output = analysis(graph);
    let record = observation(&output, id_at(graph, edge), id_at(graph, successor));
    let plan = prepare_suggestion(graph, record, &mut runner())
        .unwrap()
        .accepted()
        .unwrap();
    let [added] = plan.added().try_into().unwrap();
    plan.commit(graph).unwrap();
    added.position()
}

// The walk of RECONNECT-EXPLAINER sections 1 to 10, entry by entry, under
// TypeCheck and Reconnect.
#[test]
fn the_explainer_walk_offers_the_documented_suggestions() {
    let title = [("title", (ValueType::Text, FieldStatus::Mandatory))];
    let declare = |ty: Type| {
        Change::Entry(EntryChange::Node(NewNode {
            ty_name: Type::TYPE_NAME.into(),
            payload: ty.to_body(),
            predecessor: None,
            intent: None,
        }))
    };
    let mut graph = LifeCoreList::default();
    append(
        &mut graph,
        &[
            configure(TypeCheckFactory::name(), ""),
            configure(ReconnectFactory::name(), ""),
        ],
    );

    // 1 and 2: two types, then a topic, three notes, and three edges.
    append(
        &mut graph,
        &[
            declare(Type::new(
                "Note",
                &title,
                &["Cites", "About"],
                &["Cites"],
                &[],
            )),
            declare(Type::new("Topic", &title, &[], &["About"], &[])),
        ],
    );
    append(
        &mut graph,
        &[
            root("Topic", "Append-only graphs"),
            root("Note", "Edges hold exact revisions"),
            root("Note", "Supersession keeps history"),
            root("Note", "Reading list"),
            edge("Cites", Added(1), Added(2)),
            edge("About", Added(1), Added(0)),
            edge("Cites", Added(3), Added(1)),
        ],
    );
    assert_eq!(graph.view().len(), 9);
    assert!(analysis(&graph).suggestions().next().is_none());

    // 3: revising note 3 offers all three of its edges.
    let changes = [revise("Note", "never trees", existing_at(&graph, 3))];
    append(&mut graph, &changes);
    assert_eq!(suggested(&graph, 9), [(6, 9, 4), (7, 9, 2), (8, 5, 9)]);

    // 4 and 5: applying two, then a chain copies from note 9 only.
    assert_eq!(accept(&mut graph, 6, 9), 10);
    assert_eq!(accept(&mut graph, 7, 9), 11);
    let changes = [revise("Note", "or roots", existing_at(&graph, 9))];
    append(&mut graph, &changes);
    assert_eq!(suggested(&graph, 12), [(10, 12, 4), (11, 12, 2)]);
    assert_eq!(accept(&mut graph, 10, 12), 13);
    assert_eq!(accept(&mut graph, 11, 12), 14);

    // 6: a consecutive split offers both branches everything.
    let changes = [
        revise(
            "Note",
            "Edges hold exact revisions",
            existing_at(&graph, 12),
        ),
        branch(
            "Note",
            "Nothing resolves to a tree root",
            existing_at(&graph, 12),
        ),
    ];
    append(&mut graph, &changes);
    assert_eq!(suggested(&graph, 15), [(13, 15, 4), (14, 15, 2)]);
    assert_eq!(suggested(&graph, 16), [(13, 16, 4), (14, 16, 2)]);
    assert_eq!(accept(&mut graph, 13, 15), 17);
    assert_eq!(accept(&mut graph, 14, 15), 18);
    assert_eq!(accept(&mut graph, 14, 16), 19);

    // 7: the topic's revision is offered the edges whose note is current;
    // the note's revision then loses its edge to the superseded topic.
    let changes = [revise("Topic", "typed graphs", existing_at(&graph, 2))];
    append(&mut graph, &changes);
    assert_eq!(suggested(&graph, 20), [(18, 15, 20), (19, 16, 20)]);
    let changes = [revise("Note", "at both ends", existing_at(&graph, 15))];
    append(&mut graph, &changes);
    assert_eq!(suggested(&graph, 21), [(17, 21, 4)]);

    // 8: both endpoints and the repair in one plan.
    let changes = [
        revise("Note", "root or leaf", existing_at(&graph, 16)),
        revise("Topic", "with history", existing_at(&graph, 20)),
        edge("About", Added(0), Added(1)),
    ];
    append(&mut graph, &changes);
    for newest in 22..=24 {
        assert!(suggested(&graph, newest).is_empty());
    }

    // 9: recreating `(17, 21)` by hand makes applying it a duplicate.
    let changes = [edge(
        "Cites",
        existing_at(&graph, 21),
        existing_at(&graph, 4),
    )];
    append(&mut graph, &changes);
    let output = analysis(&graph);
    let record = observation(&output, id_at(&graph, 17), id_at(&graph, 21));
    assert!(matches!(
        prepare_suggestion(&graph, record, &mut runner()),
        Err(suggestion::Error::Check(transaction::Error::Core(
            LifeCoreError::BasicValidationFailure(BasicValidationFailure::Duplicate { .. })
        )))
    ));

    // 10: a late branch is offered only what its sibling, note 27, lacks.
    let changes = [edge(
        "Cites",
        existing_at(&graph, 5),
        existing_at(&graph, 21),
    )];
    append(&mut graph, &changes);
    let changes = [revise("Note", "always", existing_at(&graph, 21))];
    append(&mut graph, &changes);
    assert_eq!(suggested(&graph, 27), [(25, 27, 4), (26, 5, 27)]);
    assert_eq!(accept(&mut graph, 25, 27), 28);
    let changes = [branch("Note", "structural", existing_at(&graph, 21))];
    append(&mut graph, &changes);
    assert_eq!(suggested(&graph, 29), [(26, 5, 29)]);

    // Every suggestion of the walk is one sticky observation at its
    // successor's prefix.
    let output = analysis(&graph);
    assert!(!output.has_error());
    let successors = output
        .suggestions()
        .map(|observation| {
            let EvaluatedView::Prefixes { first, last } = observation.evaluated_view else {
                panic!("a suggestion is judged at a prefix");
            };
            assert!(observation.diagnostic.sticky);
            assert_eq!(first, last);
            first.end_exclusive - 1
        })
        .collect::<Vec<_>>();
    assert_eq!(
        successors,
        [9, 9, 9, 12, 12, 15, 15, 16, 16, 20, 20, 21, 27, 27, 29]
    );
}
