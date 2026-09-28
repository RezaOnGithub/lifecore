use super::super::{Change, Checked, Error, NewEdge, NewNode, Reference, RevisionIntent, prepare};
use crate::{Body, LifeCoreList, analysis::AnalysisRunner, transaction::Reference::Added};

fn node(name: &str) -> Change {
    Change::AddNode(NewNode {
        ty_name: name.into(),
        body: Body::default(),
        parent: None,
        intent: None,
    })
}

fn plan() -> Vec<Change> {
    vec![
        node("A"),
        node("B"),
        Change::AddEdge(NewEdge {
            from: Added(0),
            ty_name: "link".into(),
            to: Added(1),
        }),
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
    let checked = prepare(&graph, &plan(), &mut runner)
        .unwrap()
        .accepted()
        .unwrap();
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
    let transaction = prepare(&graph, &plan(), &mut AnalysisRunner::default())
        .unwrap()
        .accepted()
        .unwrap();
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
    let checked = prepare(&graph, &plan(), &mut AnalysisRunner::default())
        .unwrap()
        .accepted()
        .unwrap();
    prepare(&graph, &[node("Other")], &mut AnalysisRunner::default())
        .unwrap()
        .accepted()
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
        .accepted()
        .unwrap()
        .commit(&mut graph)
        .unwrap();
    let original = serde_json::to_value(&graph).unwrap();
    let mut variants = vec![];
    let mut changed = original.clone();
    changed["id"] = serde_json::json!(uuid::Uuid::new_v4());
    variants.push(changed);
    let mut changed = original.clone();
    changed["entries"][0]["Node"]["body"] = serde_json::json!({"title": "changed"});
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
        let transaction = prepare(&graph, &[], &mut runner)
            .unwrap()
            .accepted()
            .unwrap();
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
        let transaction = prepare(&graph, &[], &mut runner)
            .unwrap()
            .accepted()
            .unwrap();
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
        .accepted()
        .unwrap()
        .commit(&mut graph)
        .unwrap();
    let transaction = prepare(&graph, &[node("Other")], &mut runner)
        .unwrap()
        .accepted()
        .unwrap();
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
        .accepted()
        .unwrap()
        .commit(&mut graph)
        .unwrap();
    let original = serde_json::to_value(&graph).unwrap();
    let mut variants = vec![];
    let mut changed = original.clone();
    changed["entries"][2]["Edge"]["from"]["position"] = serde_json::json!("9999");
    variants.push(changed);
    let mut changed = original.clone();
    changed["entries"][0]["Node"]["parent"] =
        serde_json::to_value(graph.view().at(1).unwrap().id()).unwrap();
    variants.push(changed);
    let mut changed = original.clone();
    changed["entries"][0]["Node"]["id"]["position"] = serde_json::json!("2");
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
    let first = prepare(&graph, &[node("First")], &mut runner)
        .unwrap()
        .accepted()
        .unwrap();
    let second = prepare(&graph, &[node("Second")], &mut runner)
        .unwrap()
        .accepted()
        .unwrap();
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
        .accepted()
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
    ) -> Result<Vec<crate::analysis::Message>, String> {
        Ok(Default::default())
    }
    fn analyze(
        &self,
        _view: crate::View<'_>,
        _inputs: &crate::analysis::EvaluationInputs,
    ) -> Result<Vec<crate::analysis::Message>, String> {
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
    let old = prepare(&graph, &[node("Old")], &mut runner)
        .unwrap()
        .accepted()
        .unwrap();
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
    .accepted()
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
        .accepted()
        .unwrap()
        .commit(&mut graph)
        .unwrap();
    assert_eq!(graph.view().len(), 1);
    assert_eq!(graph.analysis_suite(), snapshot.analysis_suite());
}

fn revision(name: &str, parent: Reference, intent: Option<RevisionIntent>) -> Change {
    Change::AddNode(NewNode {
        ty_name: name.into(),
        body: Body::default(),
        parent: Some(parent),
        intent,
    })
}

#[test]
fn revision_intent_is_checked_immediately_before_each_append() {
    use super::super::Reference::Existing;
    let mut graph = LifeCoreList::default();
    let mut runner = AnalysisRunner::default();
    let committed = prepare(&graph, &[node("A")], &mut runner)
        .unwrap()
        .accepted()
        .unwrap();
    let root = committed.added()[0];
    committed.commit(&mut graph).unwrap();
    let before = graph.clone();
    let intent = |changes: &[Change]| prepare(&graph, changes, &mut AnalysisRunner::default());
    let mut root_with_intent = node("C");
    let Change::AddNode(new) = &mut root_with_intent else {
        unreachable!()
    };
    new.intent = Some(RevisionIntent::Branch);
    assert!(matches!(
        intent(&[node("B"), root_with_intent]),
        Err(Error::UnexpectedIntent { operation: 1 })
    ));
    assert!(matches!(
        intent(&[revision("A2", Existing(root), None)]),
        Err(Error::MissingIntent { operation: 0 })
    ));
    //A Before any successor exists, both intents accept the current root.
    for kind in [RevisionIntent::Revise, RevisionIntent::Branch] {
        assert!(intent(&[revision("A2", Existing(root), Some(kind))]).is_ok());
    }
    //A After an earlier operation in the same plan supersedes the root, the
    //A intermediate prefix rejects `revise` but still permits `branch`.
    let first = revision("A2", Existing(root), Some(RevisionIntent::Revise));
    assert!(matches!(
        intent(&[
            first.clone(),
            revision("A3", Existing(root), Some(RevisionIntent::Revise))
        ]),
        Err(Error::RevisesSuperseded { operation: 1 })
    ));
    let split = intent(&[
        first.clone(),
        revision("A3", Existing(root), Some(RevisionIntent::Branch)),
    ])
    .unwrap()
    .accepted()
    .unwrap();
    assert_eq!(split.view().fresh_successors(root).count(), 2);
    //A Revising the plan's own successor is valid at its later prefix.
    assert!(
        intent(&[
            first,
            revision("A3", Added(0), Some(RevisionIntent::Revise))
        ])
        .is_ok()
    );
    assert_eq!(graph, before);
    //A After a committed successor, the same revision fails against the new base
    //A while a branch from the superseded root still succeeds.
    prepare(
        &graph,
        &[revision("A2", Existing(root), Some(RevisionIntent::Revise))],
        &mut runner,
    )
    .unwrap()
    .accepted()
    .unwrap()
    .commit(&mut graph)
    .unwrap();
    let after = graph.clone();
    let intent = |changes: &[Change]| prepare(&graph, changes, &mut AnalysisRunner::default());
    assert!(matches!(
        intent(&[revision("A3", Existing(root), Some(RevisionIntent::Revise))]),
        Err(Error::RevisesSuperseded { operation: 0 })
    ));
    assert!(intent(&[revision("A3", Existing(root), Some(RevisionIntent::Branch))]).is_ok());
    assert_eq!(graph, after);
}

//A Emits a warning at every prefix and an inconsistency at the configured one.
struct BoundaryAnalyzer(Option<usize>);
impl crate::analysis::AnalyzerFactory for BoundaryAnalyzer {
    fn analyzer_name(&self) -> &'static str {
        "Boundary"
    }
    fn analyzer_with_config(
        &mut self,
        config: &str,
    ) -> Result<Box<dyn crate::analysis::Analyzer>, crate::LifeCoreError> {
        Ok(Box::new(Self(config.parse().ok())))
    }
}
impl crate::analysis::Analyzer for BoundaryAnalyzer {
    fn version(&self) -> semver::Version {
        semver::Version::new(1, 0, 0)
    }
    fn initialize(
        &mut self,
        _inputs: &crate::analysis::EvaluationInputs,
    ) -> Result<Vec<crate::analysis::Message>, String> {
        Ok(vec![])
    }
    fn analyze(
        &self,
        view: crate::View<'_>,
        _inputs: &crate::analysis::EvaluationInputs,
    ) -> Result<Vec<crate::analysis::Message>, String> {
        use crate::analysis::{AnalyzerCertificate, Message, MessageKind};
        let message = |kind, certificate: &str| Message {
            analyzer_certificate: AnalyzerCertificate(certificate.into()),
            subjects: Default::default(),
            offending_event: view.location(None),
            relevance: Default::default(),
            message: certificate.into(),
            kind,
            sticky: false,
        };
        let mut output = vec![message(MessageKind::Warning, "advisory")];
        if self.0 == Some(view.len()) {
            output.push(message(MessageKind::Inconsistency, "blocked"));
        }
        Ok(output)
    }
}

#[test]
fn rejections_keep_the_analysis_but_no_candidate_graph() {
    use crate::analysis::{EvaluatedView, MessageType};
    let mut graph = LifeCoreList::default();
    let mut runner = AnalysisRunner::default();
    runner
        .register_analyzer_kind(Box::new(BoundaryAnalyzer(None)))
        .unwrap();
    prepare(&graph, &[node("Base")], &mut runner)
        .unwrap()
        .accepted()
        .unwrap()
        .commit(&mut graph)
        .unwrap();
    let before = graph.clone();
    //A Reject the base boundary, an intermediate boundary, and the final one.
    for rejected_at in [1, 2, 4] {
        let mut changes = plan();
        changes.push(Change::AddAnalyzer {
            name: "Boundary".into(),
            config: rejected_at.to_string(),
        });
        let Checked::Rejected(rejection) = prepare(&graph, &changes, &mut runner).unwrap() else {
            panic!("an inconsistent candidate must not become committable")
        };
        //A Three staged entries and one configuration change, each mapped.
        let operations = rejection.operations();
        assert_eq!(operations.len(), 4);
        assert_eq!(operations[3], None);
        assert_eq!(
            operations[..3]
                .iter()
                .map(|id| id.unwrap().position())
                .collect::<Vec<_>>(),
            [1, 2, 3]
        );
        let analysis = rejection.analysis();
        assert_eq!(analysis.view.end_exclusive, 4);
        assert_eq!(analysis.view.database, graph.view().id().database);
        assert_eq!(
            analysis.implementations[0].configuration,
            rejected_at.to_string()
        );
        //A Advisories at every boundary stay alongside the blocking finding.
        assert_eq!(analysis.warnings().count(), 5);
        let blocked = analysis.errors().collect::<Vec<_>>();
        assert_eq!(blocked.len(), 1);
        assert_eq!(blocked[0].key.kind, MessageType::Inconsistency);
        let EvaluatedView::Prefix(at) = blocked[0].evaluated_view else {
            panic!("the finding names its prefix")
        };
        assert_eq!(at.end_exclusive, rejected_at);
        //A Prefixes up to the base are the committed history itself.
        if rejected_at <= 1 {
            assert_eq!(at, graph.view().prefix(rejected_at).unwrap().id());
        }
        assert_eq!(graph, before);
    }
    let clean = prepare(&graph, &plan(), &mut runner)
        .unwrap()
        .accepted()
        .unwrap();
    assert!(!clean.analysis().has_error());
    let prepared = clean;
    assert_eq!(prepared.analysis().warnings().count(), 0);
    assert_eq!(prepared.view().len(), 4);
    assert_eq!(prepared.analysis().view, prepared.view().id());
    prepared.commit(&mut graph).unwrap();
    assert_eq!(graph.view().len(), 4);
}
