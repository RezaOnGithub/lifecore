use crate::{
    CycleDisposition, CyclicComponent, DagCheck, DagCheckConfig, DagCheckFactory, IndexRange,
    SelectedEdge, SubgraphFilter, check_dag, cyclic_components,
};
use lifecore::{
    Body, EntryId, LifeCoreList,
    analysis::{
        AnalysisRunner, Analyzer, AnalyzerFactory, Config, EvaluatedView, MessageType, SuiteConfig,
    },
    transaction::{
        Change, EntryChange, NewEdge, NewNode,
        Reference::{Added, Existing},
        RevisionIntent, prepare,
    },
};
use std::collections::BTreeSet;

fn config() -> DagCheckConfig {
    DagCheckConfig {
        filter: None,
        on_cycle: CycleDisposition::Warning,
        range: None,
    }
}
fn node(name: &str) -> Change {
    Change::Entry(EntryChange::Node(NewNode {
        ty_name: name.into(),
        payload: Body::new(),
        predecessor: None,
        intent: None,
    }))
}
fn edge(a: usize, b: usize) -> Change {
    Change::Entry(EntryChange::Edge(NewEdge {
        from: Added(a),
        ty_name: "link".into(),
        to: Added(b),
    }))
}
fn graph() -> (LifeCoreList, Vec<EntryId>) {
    let mut graph = LifeCoreList::default();
    let plan = prepare(
        &graph,
        &[
            node("A"),
            node("B"),
            node("C"),
            edge(0, 1),
            edge(1, 2),
            edge(2, 0),
        ],
        &mut AnalysisRunner::default(),
    )
    .unwrap()
    .accepted()
    .unwrap();
    let ids = plan.added().to_vec();
    plan.commit(&mut graph).unwrap();
    (graph, ids)
}

#[test]
fn strict_json_configuration_preserves_set_and_inclusive_range_semantics() {
    assert!(DagCheckConfig::from_json(r#"{"on_cycle":"warning","extra":true}"#).is_err());
    assert!(
        DagCheckConfig::from_json(r#"{"on_cycle":"warning","range":{"from":3,"through":2}}"#)
            .is_err()
    );
    assert!(DagCheckConfig::from_json(r#"{"on_cycle":"unknown"}"#).is_err());
    let parsed = DagCheckConfig::from_json(
        r#"{"on_cycle":"warning","filter":{"node_types":["B","A","A"]}}"#,
    )
    .unwrap();
    assert_eq!(
        parsed.filter.unwrap().node_types.unwrap(),
        BTreeSet::from(["A".into(), "B".into()])
    );
}

#[test]
fn checker_groups_cycles_and_algorithm_handles_self_loops() {
    let (graph, ids) = graph();
    assert_eq!(
        check_dag(graph.view(), &config()).components,
        vec![CyclicComponent {
            nodes: ids[..3].to_vec(),
            edges: ids[3..].to_vec()
        }]
    );
    assert!(
        check_dag(graph.view().prefix(5).unwrap(), &config())
            .components
            .is_empty()
    );
    // Storage rejects self-edges; retain the SCC algorithm's self-loop case
    // directly.
    let selected = [SelectedEdge {
        index: ids[3],
        from: ids[0],
        to: ids[0],
    }];
    assert_eq!(
        cyclic_components(&BTreeSet::from([ids[0]]), &selected)[0].nodes,
        vec![ids[0]]
    );
}

#[test]
fn filters_select_an_induced_graph_and_stale_endpoints_remove_cycles() {
    let (mut graph, ids) = graph();
    let mut filtered = config();
    filtered.filter = Some(SubgraphFilter {
        node_types: Some(BTreeSet::from(["A".into(), "B".into()])),
        edge_types: None,
    });
    assert!(check_dag(graph.view(), &filtered).components.is_empty());
    filtered.filter = Some(SubgraphFilter {
        node_types: None,
        edge_types: Some(BTreeSet::new()),
    });
    assert!(check_dag(graph.view(), &filtered).components.is_empty());
    prepare(
        &graph,
        &[Change::Entry(EntryChange::Node(NewNode {
            ty_name: "A2".into(),
            payload: Body::new(),
            predecessor: Some(Existing(ids[0])),
            intent: Some(RevisionIntent::Branch),
        }))],
        &mut AnalysisRunner::default(),
    )
    .unwrap()
    .accepted()
    .unwrap()
    .commit(&mut graph)
    .unwrap();
    assert!(check_dag(graph.view(), &config()).components.is_empty());
    assert_eq!(
        check_dag(graph.view().prefix(6).unwrap(), &config())
            .components
            .len(),
        1
    );
}

#[test]
fn index_range_selects_prefixes_without_cropping_membership() {
    let (graph, _) = graph();
    let mut config = config();
    config.range = Some(IndexRange {
        from: Some(5),
        through: Some(5),
    });
    assert!(!check_dag(graph.view().prefix(5).unwrap(), &config).checked);
    assert_eq!(check_dag(graph.view(), &config).components.len(), 1);
    assert!(!check_dag(graph.view().prefix(0).unwrap(), &config).checked);
}

#[test]
fn diagnostics_name_the_whole_component_and_focus_on_its_least_node() {
    let (graph, ids) = graph();
    // Both dispositions focus on the least node, not on the entry that closed
    // the cycle, so the focus is the same at every prefix.
    for (on_cycle, ty) in [
        (CycleDisposition::Warning, MessageType::Warning),
        (CycleDisposition::Inconsistency, MessageType::Inconsistency),
    ] {
        let analyzer = DagCheck {
            config: DagCheckConfig {
                on_cycle,
                ..config()
            },
        };
        let output = analyzer.analyze(graph.view(), &Default::default()).unwrap();
        assert_eq!(output.len(), 1);
        assert_eq!(output[0].ty(), ty);
        assert_eq!(output[0].diagnostic.focus, Some(ids[0]));
        assert_eq!(
            output[0].diagnostic.subjects,
            ids.iter().copied().collect::<BTreeSet<_>>()
        );
    }
}

#[test]
fn factory_and_replay_respect_the_supplied_view() {
    let (graph, _) = graph();
    let mut factory = DagCheckFactory;
    assert!(factory.analyzer_with_config("bad").is_err());
    let mut runner = AnalysisRunner::default();
    runner.register_analyzer_kind(Box::new(factory)).unwrap();
    let suite = SuiteConfig(vec![Config {
        name: DagCheckFactory::name().into(),
        configuration: r#"{"on_cycle":"inconsistency"}"#.into(),
    }]);
    assert!(
        !runner
            .run_view(graph.view().prefix(5).unwrap(), &suite)
            .unwrap()
            .has_error()
    );
    assert!(runner.run_view(graph.view(), &suite).unwrap().has_error());
}

#[test]
fn transaction_rejects_a_historical_cycle_even_if_the_final_graph_is_acyclic() {
    let graph = LifeCoreList::default();
    let mut runner = AnalysisRunner::default();
    runner
        .register_analyzer_kind(Box::new(DagCheckFactory))
        .unwrap();
    let changes = [
        node("A"),
        node("B"),
        edge(0, 1),
        edge(1, 0),
        Change::Entry(EntryChange::Node(NewNode {
            ty_name: "A2".into(),
            payload: Body::new(),
            predecessor: Some(Added(0)),
            intent: Some(RevisionIntent::Revise),
        })),
        Change::AddAnalyzer {
            name: DagCheckFactory::name().into(),
            configuration: r#"{"on_cycle":"inconsistency"}"#.into(),
        },
    ];
    let checked = prepare(&graph, &changes, &mut runner).unwrap();
    // Only the prefix that closes the cycle is inconsistent; the revision after
    // it cannot excuse that boundary.
    let boundaries = checked
        .analysis()
        .errors()
        .map(|o| match o.evaluated_view {
        EvaluatedView::Prefixes { first, last } => (first.end_exclusive, last.end_exclusive),
        EvaluatedView::Initialization => panic!("cycles have a prefix"),
        })
        .collect::<Vec<_>>();
    assert_eq!(boundaries, vec![(4, 4)]);
    assert!(checked.accepted().is_err());
    assert!(graph.view().is_empty());
}

#[test]
fn cycle_keys_preserve_configuration_and_group_observations_without_inventing_resolution() {
    use lifecore::analysis::presentation::{
        DismissalReason, Dismissals, ReportOptions, ReportSelection,
    };
    let (mut graph, ids) = graph();
    prepare(&graph, &[node("Unrelated")], &mut AnalysisRunner::default())
        .unwrap()
        .accepted()
        .unwrap()
        .commit(&mut graph)
        .unwrap();
    let config = |raw: &str| {
        SuiteConfig(vec![Config {
            name: DagCheckFactory::name().into(),
            configuration: raw.into(),
        }])
    };
    let mut runner = AnalysisRunner::default();
    runner
        .register_analyzer_kind(Box::new(DagCheckFactory))
        .unwrap();
    let plain = runner
        .run_view(graph.view(), &config(r#"{"on_cycle":"warning"}"#))
        .unwrap();
    let normalized = runner
        .run_view(
            graph.view(),
            &config(r#"{ "range": {}, "filter": {}, "on_cycle": "warning" }"#),
        )
        .unwrap();
    assert_ne!(plain, normalized);
    assert_eq!(
        plain.warnings().next().unwrap().key.analyzer_certificate,
        normalized
            .warnings()
            .next()
            .unwrap()
            .key
            .analyzer_certificate
    );
    assert_eq!(
        plain.implementations[0].version,
        env!("CARGO_PKG_VERSION")
            .parse::<semver::Version>()
            .unwrap()
    );
    assert_eq!(
        normalized.implementations[0].configuration,
        r#"{ "range": {}, "filter": {}, "on_cycle": "warning" }"#
    );
    let mut dismissals = Dismissals::default();
    let groups = plain.groups(&dismissals, ReportOptions::default());
    assert_eq!(groups.len(), 1);
    // The cycle closes at prefix 6 and persists through the unrelated append.
    assert_eq!(groups[0].observations.len(), 1);
    assert_eq!(
        groups[0].observations[0].evaluated_view,
        EvaluatedView::Prefixes {
            first: graph.view().prefix(6).unwrap().id(),
            last: graph.view().id(),
        }
    );
    assert_eq!(
        groups[0].observations[0].diagnostic.subjects,
        ids.iter().copied().collect()
    );
    let key = groups[0].key.clone();
    dismissals
        .dismiss(
            key.clone(),
            DismissalReason::Acknowledged,
            Some("expected loop".into()),
        )
        .unwrap();
    assert_eq!(
        normalized
            .groups(&dismissals, ReportOptions::default())
            .len(),
        1
    );
    assert!(
        plain
            .groups(&dismissals, ReportOptions::default())
            .is_empty()
    );
    let filtered = runner
        .run_view(
            graph.view(),
            &config(r#"{"on_cycle":"warning","filter":{"node_types":["C","A","B","A"]}}"#),
        )
        .unwrap();
    let filtered_again = runner
        .run_view(
            graph.view(),
            &config(r#"{"filter":{"node_types":["A","B","C"]},"on_cycle":"warning"}"#),
        )
        .unwrap();
    assert_ne!(filtered, filtered_again);
    assert_eq!(
        filtered.warnings().next().unwrap().key.analyzer_certificate,
        filtered_again
            .warnings()
            .next()
            .unwrap()
            .key
            .analyzer_certificate
    );
    assert_ne!(filtered.warnings().next().unwrap().key, key);
    prepare(
        &graph,
        &[Change::Entry(EntryChange::Node(NewNode {
            ty_name: "A2".into(),
            payload: Body::new(),
            predecessor: Some(Existing(ids[0])),
            intent: Some(RevisionIntent::Branch),
        }))],
        &mut AnalysisRunner::default(),
    )
    .unwrap()
    .accepted()
    .unwrap()
    .commit(&mut graph)
    .unwrap();
    let after = runner
        .run_view(graph.view(), &config(r#"{"on_cycle":"warning"}"#))
        .unwrap();
    assert!(dismissals.restore(&key));
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
                    ..Default::default()
                }
            )
            .len(),
        1
    );
    // Revising A ends the cycle at prefix 8; the range stops at 7.
    let warnings = after.warnings().collect::<Vec<_>>();
    assert_eq!(warnings.len(), 1);
    assert_eq!(
        warnings[0].evaluated_view,
        EvaluatedView::Prefixes {
            first: graph.view().prefix(6).unwrap().id(),
            last: graph.view().prefix(7).unwrap().id(),
        }
    );
}
