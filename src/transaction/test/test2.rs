use super::super::{Change, Error, Store, prepare};
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
fn storage_round_trip_no_clobber_stale_base_and_failed_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("graph.json");
    let mut store = Store::new(&path);
    let graph = LifeCoreList::default();
    let mut runner = AnalysisRunner::default();
    store.initialize(&graph, &mut runner).unwrap();
    let before = std::fs::read(&path).unwrap();
    assert!(store.initialize(&graph, &mut runner).is_err());
    let first = prepare(&graph, &plan(), &mut runner).unwrap();
    let stale = prepare(&graph, &[node("Other")], &mut runner).unwrap();
    store.fail_replace = true;
    assert!(store.commit(first).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    store.fail_replace = false;
    let saved = store
        .commit(prepare(&graph, &plan(), &mut runner).unwrap())
        .unwrap();
    assert!(matches!(store.commit(stale), Err(Error::StaleBase)));
    let loaded = store.load().unwrap();
    assert_eq!(
        serde_json::to_vec(&loaded).unwrap(),
        serde_json::to_vec(&saved).unwrap()
    );
    assert!(loaded.view().at(2).unwrap().as_edge().is_some());
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
fn both_commit_paths_reject_changes_without_an_append() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("graph.json");
    let store = Store::new(&path);
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

        let bytes = serde_json::to_vec(&receiver).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let transaction = prepare(&graph, &[], &mut runner).unwrap();
        assert!(matches!(store.commit(transaction), Err(Error::StaleBase)));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn fingerprint_includes_analyzer_names_configuration_and_order() {
    use super::super::StateFingerprint;
    use crate::analysis::Config;

    let mut graph = LifeCoreList::default();
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
    let original = StateFingerprint::of(&graph).unwrap();
    let mut renamed = graph.clone();
    renamed.suite.0[0].name = "Renamed".into();
    let mut reconfigured = graph.clone();
    reconfigured.suite.0[0].config = "changed".into();
    let mut reordered = graph.clone();
    reordered.suite.0.reverse();
    for changed in [renamed, reconfigured, reordered] {
        assert_eq!(changed.view().id(), graph.view().id());
        assert_ne!(StateFingerprint::of(&changed).unwrap(), original);
    }
}

#[test]
fn file_formatting_does_not_invalidate_a_prepared_transaction() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("graph.json");
    let store = Store::new(&path);
    let graph = LifeCoreList::default();
    let mut runner = AnalysisRunner::default();
    store.initialize(&graph, &mut runner).unwrap();
    let transaction = prepare(&graph, &plan(), &mut runner).unwrap();
    let original = std::fs::read(&path).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&original).unwrap();
    //A Value reorders object keys; compact encoding also removes file whitespace.
    let reformatted = serde_json::to_vec(&value).unwrap();
    assert_ne!(reformatted, original);
    std::fs::write(&path, reformatted).unwrap();
    let expected = transaction.candidate.clone();
    assert_eq!(store.commit(transaction).unwrap(), expected);
    assert_eq!(store.load().unwrap(), expected);
}

#[test]
fn malformed_persisted_reference_is_diagnosed_without_panicking() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("graph.json");
    let mut graph = LifeCoreList::default();
    prepare(&graph, &plan(), &mut AnalysisRunner::default())
        .unwrap()
        .commit(&mut graph)
        .unwrap();
    let mut encoded = serde_json::to_value(&graph).unwrap();
    encoded["entries"][2]["Edge"]["from"]["position"] = serde_json::json!(9999);
    std::fs::write(&path, serde_json::to_vec(&encoded).unwrap()).unwrap();
    assert!(Store::new(path).load().is_err());
}

#[test]
fn concurrent_writers_publish_only_one_candidate_for_the_same_base() {
    use std::sync::{Arc, Barrier};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("graph.json");
    let graph = LifeCoreList::default();
    let mut runner = AnalysisRunner::default();
    Store::new(&path).initialize(&graph, &mut runner).unwrap();
    let first = prepare(&graph, &[node("First")], &mut runner).unwrap();
    let second = prepare(&graph, &[node("Second")], &mut runner).unwrap();
    let barrier = Arc::new(Barrier::new(2));
    let handles = [first, second]
        .into_iter()
        .map(|plan| {
            let barrier = barrier.clone();
            let path = path.clone();
            std::thread::spawn(move || {
                barrier.wait();
                Store::new(path).commit(plan)
            })
        })
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(Error::StaleBase)))
            .count(),
        1
    );
    assert_eq!(Store::new(path).load().unwrap().view().len(), 1);
}

#[test]
fn imported_configuration_cannot_bypass_the_initialization_gate() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("graph.json");
    let mut graph = LifeCoreList::default();
    graph.suite.0.push(crate::analysis::Config {
        name: "Unavailable".into(),
        config: String::new(),
    });
    assert!(
        Store::new(&path)
            .initialize(&graph, &mut AnalysisRunner::default())
            .is_err()
    );
    assert!(!path.exists());
}

#[test]
fn directory_sync_failure_reports_publication_instead_of_claiming_rollback() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("graph.json");
    let mut store = Store::new(path);
    let graph = LifeCoreList::default();
    let mut runner = AnalysisRunner::default();
    store.initialize(&graph, &mut runner).unwrap();
    let plan = prepare(&graph, &[node("A")], &mut runner).unwrap();
    store.fail_directory_sync = true;
    assert!(matches!(store.commit(plan), Err(Error::Published(_))));
    assert_eq!(store.load().unwrap().view().len(), 1);
}

#[test]
fn missing_analyzer_does_not_disable_committed_history_inspection() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("graph.json");
    let mut graph = LifeCoreList::default();
    prepare(&graph, &[node("A")], &mut AnalysisRunner::default())
        .unwrap()
        .commit(&mut graph)
        .unwrap();
    graph.suite.0.push(crate::analysis::Config {
        name: "Unavailable".into(),
        config: String::new(),
    });
    std::fs::write(&path, serde_json::to_vec(&graph).unwrap()).unwrap();
    let loaded = Store::new(path).load().unwrap();
    assert_eq!(loaded.view().nodes().next().unwrap().ty_name, "A");
    assert!(prepare(&loaded, &[], &mut AnalysisRunner::default()).is_err());
}
