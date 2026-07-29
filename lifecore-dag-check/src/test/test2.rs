use std::collections::BTreeSet;

use lifecore::{
    Body, Context, DiagnosticLocation, EntryIndex, LifeCore, LifeCoreAccess, LifeCoreList, Payload,
    analysis::{AnalysisRunner, AnalyzerFactory, LifeCoreAnalysisHarness},
};

use crate::{
    CycleDisposition, CyclicComponent, DagCheck, DagCheckConfig, DagCheckFactory, IndexRange,
    SubgraphFilter, check_dag,
};

fn warning_config() -> DagCheckConfig {
    DagCheckConfig {
        filter: None,
        on_cycle: CycleDisposition::Warning,
        range: None,
    }
}

fn cycle_graph() -> (
    LifeCoreList,
    lifecore::Node,
    lifecore::Node,
    lifecore::Edge,
    lifecore::Edge,
) {
    let mut graph = LifeCoreList::default();
    let first = graph
        .add_mock("Keep", Body::from([("id", Payload::Number(1))]), None)
        .expect("first node appends");
    let second = graph
        .add_mock("Keep", Body::from([("id", Payload::Number(2))]), None)
        .expect("second node appends");
    let outgoing = graph
        .connect_mock(first.e, "Link", second.e)
        .expect("outgoing edge appends");
    let incoming = graph
        .connect_mock(second.e, "Link", first.e)
        .expect("incoming edge appends");
    (graph, first, second, outgoing, incoming)
}

#[test]
fn strict_json_configuration_preserves_set_and_inclusive_range_semantics() {
    let config = DagCheckConfig::from_json(
        r#"{
            "filter": {
                "node_types": ["Task", "Task"],
                "edge_types": []
            },
            "on_cycle": "warning",
            "range": {"from": 4, "through": 9}
        }"#,
    )
    .expect("configuration parses");
    assert_eq!(
        config.filter.as_ref().unwrap().node_types,
        Some(BTreeSet::from(["Task".to_string()]))
    );
    assert_eq!(
        config.filter.as_ref().unwrap().edge_types,
        Some(BTreeSet::new())
    );
    assert_eq!(
        config.range,
        Some(IndexRange {
            from: Some(4),
            through: Some(9)
        })
    );

    let all = DagCheckConfig::from_json(
        r#"{
            "filter": {"node_types": null, "edge_types": null},
            "on_cycle": "inconsistency",
            "range": null
        }"#,
    )
    .expect("null optional values parse");
    assert_eq!(all.filter, Some(SubgraphFilter::default()));
    assert_eq!(all.range, None);

    for invalid in [
        r#"{"on_cycle":"warning","unknown":true}"#,
        r#"{"filter":{"unknown":[]},"on_cycle":"warning"}"#,
        r#"{"on_cycle":"notice"}"#,
        r#"{"on_cycle":"warning","range":{"from":5,"through":4}}"#,
        r#"{}"#,
    ] {
        assert!(
            DagCheckConfig::from_json(invalid).is_err(),
            "{invalid} should be rejected"
        );
    }
}

#[test]
fn checker_reports_self_loops_and_groups_overlapping_cycles_by_scc() {
    let mut graph = LifeCoreList::default();
    let first = graph
        .add_mock("Node", Body::from([("id", Payload::Number(1))]), None)
        .expect("first node appends");
    let second = graph
        .add_mock("Node", Body::from([("id", Payload::Number(2))]), None)
        .expect("second node appends");
    let third = graph
        .add_mock("Node", Body::from([("id", Payload::Number(3))]), None)
        .expect("third node appends");
    let fourth = graph
        .add_mock("Node", Body::from([("id", Payload::Number(4))]), None)
        .expect("fourth node appends");
    let first_second = graph
        .connect_mock(first.e, "Link", second.e)
        .expect("first edge appends");
    let second_first = graph
        .connect_mock(second.e, "Link", first.e)
        .expect("second edge appends");
    let second_third = graph
        .connect_mock(second.e, "Link", third.e)
        .expect("third edge appends");
    let third_second = graph
        .connect_mock(third.e, "Link", second.e)
        .expect("fourth edge appends");
    let fourth_loop = graph
        .connect_mock(fourth.e, "Link", fourth.e)
        .expect("self-loop appends");

    let report = check_dag(&graph, fourth_loop.e.context(), &warning_config());

    assert!(report.checked);
    assert_eq!(
        report.components,
        vec![
            CyclicComponent {
                nodes: vec![first.e.index(), second.e.index(), third.e.index()],
                edges: vec![
                    first_second.e.index(),
                    second_first.e.index(),
                    second_third.e.index(),
                    third_second.e.index()
                ],
            },
            CyclicComponent {
                nodes: vec![fourth.e.index()],
                edges: vec![fourth_loop.e.index()],
            }
        ]
    );
}

#[test]
fn fresh_node_and_edge_filters_form_the_induced_subgraph() {
    let (mut graph, first, second, _, incoming) = cycle_graph();
    let outside = graph
        .add_mock("Drop", Body::default(), None)
        .expect("outside node appends");
    graph
        .connect_mock(second.e, "Other", outside.e)
        .expect("outside edge appends");

    let only_link = DagCheckConfig {
        filter: Some(SubgraphFilter {
            node_types: Some(BTreeSet::from(["Keep".to_string()])),
            edge_types: Some(BTreeSet::from(["Link".to_string()])),
        }),
        ..warning_config()
    };
    assert_eq!(
        check_dag(&graph, graph.last().unwrap().context(), &only_link)
            .components
            .len(),
        1
    );

    let no_matching_edges = DagCheckConfig {
        filter: Some(SubgraphFilter {
            node_types: Some(BTreeSet::from(["Keep".to_string()])),
            edge_types: Some(BTreeSet::from(["Other".to_string()])),
        }),
        ..warning_config()
    };
    assert!(
        check_dag(&graph, graph.last().unwrap().context(), &no_matching_edges)
            .components
            .is_empty()
    );

    graph
        .add_mock("Keep", Body::default(), Some(first.e))
        .expect("successor appends");
    let report = check_dag(&graph, graph.last().unwrap().context(), &only_link);
    assert!(report.components.is_empty());
    assert!(graph.is_stale(incoming.e.absolute()).unwrap());
}

#[test]
fn index_range_selects_prefixes_without_cropping_graph_membership() {
    let (graph, _, _, _, incoming) = cycle_graph();
    let at_cycle = incoming.e.context();
    let latest = incoming.e.index().0;
    let exact = DagCheckConfig {
        range: Some(IndexRange {
            from: Some(latest),
            through: Some(latest),
        }),
        ..warning_config()
    };
    let report = check_dag(&graph, at_cycle, &exact);
    assert!(report.checked);
    assert_eq!(report.components.len(), 1);
    let unbounded_report = check_dag(&graph, Context::universe(), &exact);
    assert!(unbounded_report.checked);
    assert_eq!(unbounded_report.components, report.components);

    let before = DagCheckConfig {
        range: Some(IndexRange {
            from: None,
            through: Some(latest - 1),
        }),
        ..warning_config()
    };
    let report = check_dag(&graph, at_cycle, &before);
    assert!(!report.checked);
    assert!(report.components.is_empty());

    let empty = check_dag(&graph, EntryIndex(0).before(), &warning_config());
    assert!(!empty.checked);
}

#[test]
fn warning_and_inconsistency_diagnostics_follow_location_and_relevance_policy() {
    let (mut graph, first, second, outgoing, incoming) = cycle_graph();
    let later = graph
        .add_mock("Unrelated", Body::default(), None)
        .expect("later node appends");
    let warning_json = r#"{"on_cycle":"warning","range":{"from":3}}"#;
    graph.add_analyzer(DagCheckFactory::<LifeCoreList>::name(), warning_json);
    let mut runner = AnalysisRunner::default();
    runner
        .register_analyzer_kind(Box::new(DagCheckFactory::<LifeCoreList>::default()))
        .expect("factory registers");

    let output = runner.run(&graph).expect("analysis succeeds");
    let warnings = output
        .0
        .into_iter()
        .filter_map(|message| message.try_as_warning())
        .collect::<Vec<_>>();
    assert_eq!(warnings.len(), 2);
    assert_eq!(
        warnings[0].offending_event,
        DiagnosticLocation::Entry(incoming.e.context().look_at(first.e))
    );
    assert_eq!(
        warnings[1].offending_event,
        DiagnosticLocation::Entry(later.e.context().look_at(first.e))
    );
    assert_eq!(
        warnings[0].relevance,
        vec![
            first.e.index(),
            second.e.index(),
            outgoing.e.index(),
            incoming.e.index()
        ]
    );
    assert!(warnings.iter().all(|warning| {
        warning.system_name == "DAG_Check" && warning.system_config == warning_json
    }));

    let synthetic = CyclicComponent {
        nodes: vec![EntryIndex(0), EntryIndex(2)],
        edges: vec![EntryIndex(3)],
    };
    assert_eq!(
        DagCheck::<LifeCoreList>::warning_location(
            EntryIndex(5).before(),
            &synthetic,
            Some(EntryIndex(2))
        ),
        DiagnosticLocation::Entry(
            EntryIndex(5)
                .before()
                .look_at(EntryIndex(2).before_and_including())
        )
    );

    let (mut graph, _, _, _, incoming) = cycle_graph();
    let inconsistency_json = format!(
        r#"{{"on_cycle":"inconsistency","range":{{"from":{},"through":{}}}}}"#,
        incoming.e.index().0,
        incoming.e.index().0
    );
    graph.add_analyzer(DagCheckFactory::<LifeCoreList>::name(), &inconsistency_json);
    let mut runner = AnalysisRunner::default();
    runner
        .register_analyzer_kind(Box::new(DagCheckFactory::<LifeCoreList>::default()))
        .expect("factory registers");
    let errors = runner.run(&graph).expect("analysis succeeds").errors();
    assert_eq!(errors.len(), 1);
    assert_eq!(
        errors[0].offending_event,
        DiagnosticLocation::Context(incoming.e.context())
    );
}

#[test]
fn factory_rejects_invalid_json_with_typed_core_error_and_harness_stays_bounded() {
    let mut factory = DagCheckFactory::<LifeCoreList>::default();
    match factory.analyzer_with_config("{}") {
    Err(lifecore::LifeCoreError::InvalidAnalyzerConfiguration {
        analyzer_name,
        message,
    }) => {
        assert_eq!(analyzer_name, "DAG_Check");
        assert!(message.contains("missing field"));
    }
    _ => panic!("invalid configuration should use the typed core error"),
    }

    let (mut graph, _, _, _, incoming) = cycle_graph();
    let bound = incoming.e.context();
    graph
        .add_mock("Future", Body::default(), None)
        .expect("future node appends");
    let harness = LifeCoreAnalysisHarness::new(&graph, bound);
    let report = check_dag(&harness, Context::universe(), &warning_config());
    assert!(report.checked);
    assert_eq!(report.components.len(), 1);
    assert!(
        report.components[0]
            .nodes
            .iter()
            .all(|entry| entry.0 < bound.last().unwrap().index().0)
    );
}
