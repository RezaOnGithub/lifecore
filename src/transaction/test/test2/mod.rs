use super::super::{Change, Error, prepare};
use crate::{Body, LifeCoreList, analysis::AnalysisRunner, transaction::Reference::Added};

fn node(name: &str) -> Change {
    Change::AddNode {
        ty_name: name.into(),
        body: Body::default(),
        parent: None,
    }
}

fn plan() -> Vec<Change> {
    vec![
        node("A"),
        node("B"),
        Change::AddEdge {
            from: Added(0),
            ty_name: "link".into(),
            to: Added(1),
        },
    ]
}

#[test]
fn composite_commits_once_and_failed_suffix_leaves_memory_unchanged() {
    let mut graph = LifeCoreList::default();
    let mut runner = AnalysisRunner::default();
    let mut invalid = plan();
    invalid.push(node("A"));
    let before = serde_json::to_vec(&graph).unwrap();
    assert!(prepare(&graph, &invalid, &mut runner).is_err());
    assert_eq!(serde_json::to_vec(&graph).unwrap(), before);
    let checked = prepare(&graph, &plan(), &mut runner).unwrap();
    assert!(graph.view().is_empty());
    assert_eq!(checked.view().len(), 3);
    checked.commit(&mut graph).unwrap();
    assert_eq!(graph.view().len(), 3);
}

#[test]
fn missing_factory_does_not_publish_any_entries() {
    let graph = LifeCoreList::default();
    let mut changes = plan();
    changes.push(Change::AddAnalyzer {
        name: "missing".into(),
        config: String::new(),
    });
    assert!(prepare(&graph, &changes, &mut AnalysisRunner::default()).is_err());
    assert!(graph.view().is_empty());
}

#[test]
fn graph_round_trip_preserves_composite_entries_and_exact_identities() {
    let mut graph = LifeCoreList::default();
    let transaction = prepare(&graph, &plan(), &mut AnalysisRunner::default()).unwrap();
    let ids = transaction.added().to_vec();
    transaction.commit(&mut graph).unwrap();
    let loaded: LifeCoreList =
        serde_json::from_slice(&serde_json::to_vec(&graph).unwrap()).unwrap();
    assert_eq!(loaded, graph);
    let edge = loaded.view().edge(ids[2]).unwrap();
    assert_eq!((edge.from, edge.to), (ids[0], ids[1]));
}

#[test]
fn stale_memory_candidate_is_rejected() {
    let mut graph = LifeCoreList::default();
    let checked = prepare(&graph, &plan(), &mut AnalysisRunner::default()).unwrap();
    prepare(&graph, &[node("Other")], &mut AnalysisRunner::default())
        .unwrap()
        .commit(&mut graph)
        .unwrap();
    let before = serde_json::to_vec(&graph).unwrap();
    assert!(matches!(checked.commit(&mut graph), Err(Error::StaleBase)));
    assert_eq!(serde_json::to_vec(&graph).unwrap(), before);
}

#[test]
fn exact_base_rejects_identity_content_count_and_suite_changes() {
    let mut graph = LifeCoreList::default();
    let mut runner = AnalysisRunner::default();
    prepare(&graph, &plan(), &mut runner)
        .unwrap()
        .commit(&mut graph)
        .unwrap();
    let original = serde_json::to_value(&graph).unwrap();
    let mut variants = vec![];
    let mut changed = original.clone();
    changed["id"] = serde_json::json!(uuid::Uuid::new_v4());
    variants.push(changed);
    let mut changed = original.clone();
    changed["entries"][0]["Node"]["body"] =
        serde_json::json!({"title": {"kind": "text", "value": "changed"}});
    variants.push(changed);
    let mut changed = original.clone();
    changed["entries"][2]["Edge"]["id"]["origin"] = serde_json::json!(uuid::Uuid::new_v4());
    variants.push(changed);
    let mut changed = original.clone();
    changed["entries"].as_array_mut().unwrap().pop();
    variants.push(changed);
    let mut changed = original;
    changed["suite"] = serde_json::json!([{"name": "NewAnalyzer", "config": "{}"}]);
    variants.push(changed);

    for encoded in variants {
        let mut receiver: LifeCoreList = serde_json::from_value(encoded).unwrap();
        let before = receiver.clone();
        let transaction = prepare(&graph, &[], &mut runner).unwrap();
        assert!(matches!(
            transaction.commit(&mut receiver),
            Err(Error::StaleBase)
        ));
        assert_eq!(receiver, before);
    }
}

#[test]
fn exact_base_includes_analyzer_names_original_configuration_and_order() {
    use crate::analysis::Config;
    let mut graph = LifeCoreList::default();
    let mut runner = AnalysisRunner::default();
    for name in ["First", "Second"] {
        runner
            .register_analyzer_kind(Box::new(EmptyAnalyzer(name)))
            .unwrap();
    }
    graph.suite.0 = vec![
        Config {
            name: "First".into(),
            config: "one".into(),
        },
        Config {
            name: "Second".into(),
            config: "two".into(),
        },
    ];
    let mut renamed = graph.clone();
    renamed.suite.0[0].name = "Renamed".into();
    let mut reconfigured = graph.clone();
    reconfigured.suite.0[0].config = " one ".into();
    let mut reordered = graph.clone();
    reordered.suite.0.reverse();
    for mut receiver in [renamed, reconfigured, reordered] {
        assert_eq!(receiver.view().id(), graph.view().id());
        let before = receiver.clone();
        let transaction = prepare(&graph, &[], &mut runner).unwrap();
        assert!(matches!(
            transaction.commit(&mut receiver),
            Err(Error::StaleBase)
        ));
        assert_eq!(receiver, before);
    }
}

#[test]
fn serialization_formatting_does_not_invalidate_an_exact_base() {
    let mut graph = LifeCoreList::default();
    let mut runner = AnalysisRunner::default();
    prepare(&graph, &plan(), &mut runner)
        .unwrap()
        .commit(&mut graph)
        .unwrap();
    let transaction = prepare(&graph, &[node("Other")], &mut runner).unwrap();
    let original = serde_json::to_vec_pretty(&graph).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&original).unwrap();
    //A Value sorts object keys; compact encoding also removes whitespace.
    let reformatted = serde_json::to_vec(&value).unwrap();
    assert_ne!(reformatted, original);
    let mut receiver = serde_json::from_slice(&reformatted).unwrap();
    let expected = transaction.candidate.clone();
    transaction.commit(&mut receiver).unwrap();
    assert_eq!(receiver, expected);
}

#[test]
fn malformed_deserialization_is_rejected_before_traversal() {
    let mut graph = LifeCoreList::default();
    prepare(&graph, &plan(), &mut AnalysisRunner::default())
        .unwrap()
        .commit(&mut graph)
        .unwrap();
    let original = serde_json::to_value(&graph).unwrap();
    let mut variants = vec![];
    let mut changed = original.clone();
    changed["entries"][2]["Edge"]["from"]["position"] = serde_json::json!(9999);
    variants.push(changed);
    let mut changed = original.clone();
    changed["entries"][0]["Node"]["parent"] =
        serde_json::to_value(graph.view().at(1).unwrap().id()).unwrap();
    variants.push(changed);
    let mut changed = original.clone();
    changed["entries"][0]["Node"]["id"]["position"] = serde_json::json!(2);
    variants.push(changed);
    let mut changed = original;
    changed["id"] = serde_json::json!(uuid::Uuid::nil());
    variants.push(changed);
    for encoded in variants {
        assert!(serde_json::from_value::<LifeCoreList>(encoded).is_err());
    }
}

#[test]
fn independent_candidates_cannot_overwrite_a_committed_suffix() {
    let mut graph = LifeCoreList::default();
    let mut runner = AnalysisRunner::default();
    let first = prepare(&graph, &[node("First")], &mut runner).unwrap();
    let second = prepare(&graph, &[node("Second")], &mut runner).unwrap();
    first.commit(&mut graph).unwrap();
    let before = graph.clone();
    assert!(matches!(second.commit(&mut graph), Err(Error::StaleBase)));
    assert_eq!(graph, before);
}

#[test]
fn imported_configuration_cannot_bypass_the_preparation_gate() {
    let mut graph = LifeCoreList::default();
    graph.suite.0.push(crate::analysis::Config {
        name: "Unavailable".into(),
        config: String::new(),
    });
    let loaded: LifeCoreList =
        serde_json::from_slice(&serde_json::to_vec(&graph).unwrap()).unwrap();
    assert!(prepare(&loaded, &[], &mut AnalysisRunner::default()).is_err());
    assert_eq!(loaded, graph);
}

#[test]
fn missing_analyzer_does_not_disable_committed_history_inspection() {
    let mut graph = LifeCoreList::default();
    prepare(&graph, &[node("A")], &mut AnalysisRunner::default())
        .unwrap()
        .commit(&mut graph)
        .unwrap();
    graph.suite.0.push(crate::analysis::Config {
        name: "Unavailable".into(),
        config: String::new(),
    });
    let loaded: LifeCoreList =
        serde_json::from_slice(&serde_json::to_vec(&graph).unwrap()).unwrap();
    assert_eq!(loaded.view().nodes().next().unwrap().ty_name, "A");
    assert!(prepare(&loaded, &[], &mut AnalysisRunner::default()).is_err());
}

struct EmptyAnalyzer(&'static str);
impl crate::analysis::AnalyzerFactory for EmptyAnalyzer {
    fn analyzer_name(&self) -> &'static str {
        self.0
    }
    fn analyzer_with_config(
        &mut self,
        _config: &str,
    ) -> Result<Box<dyn crate::analysis::Analyzer>, crate::LifeCoreError> {
        Ok(Box::new(Self(self.0)))
    }
}
impl crate::analysis::Analyzer for EmptyAnalyzer {
    fn version(&self) -> semver::Version {
        semver::Version::new(1, 0, 0)
    }
    fn initialize(
        &mut self,
        _inputs: &crate::analysis::EvaluationInputs,
    ) -> Result<crate::analysis::Output, String> {
        Ok(Default::default())
    }
    fn analyze(
        &self,
        _view: crate::View<'_>,
        _inputs: &crate::analysis::EvaluationInputs,
    ) -> Result<crate::analysis::Output, String> {
        Ok(Default::default())
    }
}

#[test]
fn suite_only_commit_checks_original_suite_and_blocks_an_older_candidate() {
    let mut graph = LifeCoreList::default();
    let mut runner = AnalysisRunner::default();
    runner
        .register_analyzer_kind(Box::new(EmptyAnalyzer("First")))
        .unwrap();
    let old = prepare(&graph, &[node("Old")], &mut runner).unwrap();
    let before = graph.view().id();
    prepare(
        &graph,
        &[Change::AddAnalyzer {
            name: "First".into(),
            config: " {} ".into(),
        }],
        &mut runner,
    )
    .unwrap()
    .commit(&mut graph)
    .unwrap();
    assert_eq!(graph.view().id(), before);
    assert_eq!(graph.analysis_suite().0[0].config, " {} ");
    let snapshot = graph.clone();
    assert!(matches!(old.commit(&mut graph), Err(Error::StaleBase)));
    assert_eq!(graph, snapshot);
    prepare(&graph, &[node("New")], &mut runner)
        .unwrap()
        .commit(&mut graph)
        .unwrap();
    assert_eq!(graph.view().len(), 1);
    assert_eq!(graph.analysis_suite(), snapshot.analysis_suite());
}
