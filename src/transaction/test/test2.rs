use super::super::{Change, Error, Store, check};
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
    assert!(check(&graph, &invalid, &mut runner).is_err());
    assert_eq!(serde_json::to_vec(&graph).unwrap(), before);
    let checked = check(&graph, &plan(), &mut runner).unwrap();
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
    assert!(check(&graph, &changes, &mut AnalysisRunner::default()).is_err());
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
    let first = check(&graph, &plan(), &mut runner).unwrap();
    let stale = check(&graph, &[node("Other")], &mut runner).unwrap();
    store.fail_replace = true;
    assert!(store.commit(first).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    store.fail_replace = false;
    let saved = store
        .commit(check(&graph, &plan(), &mut runner).unwrap())
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
    let checked = check(&graph, &plan(), &mut AnalysisRunner::default()).unwrap();
    check(&graph, &[node("Other")], &mut AnalysisRunner::default())
        .unwrap()
        .commit(&mut graph)
        .unwrap();
    let before = serde_json::to_vec(&graph).unwrap();
    assert!(matches!(checked.commit(&mut graph), Err(Error::StaleBase)));
    assert_eq!(serde_json::to_vec(&graph).unwrap(), before);
}

#[test]
fn malformed_persisted_reference_is_diagnosed_without_panicking() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("graph.json");
    let mut graph = LifeCoreList::default();
    check(&graph, &plan(), &mut AnalysisRunner::default())
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
    let first = check(&graph, &[node("First")], &mut runner).unwrap();
    let second = check(&graph, &[node("Second")], &mut runner).unwrap();
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
    let plan = check(&graph, &[node("A")], &mut runner).unwrap();
    store.fail_directory_sync = true;
    assert!(matches!(store.commit(plan), Err(Error::Published(_))));
    assert_eq!(store.load().unwrap().view().len(), 1);
}

#[test]
fn missing_analyzer_does_not_disable_committed_history_inspection() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("graph.json");
    let mut graph = LifeCoreList::default();
    check(&graph, &[node("A")], &mut AnalysisRunner::default())
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
    assert!(check(&loaded, &[], &mut AnalysisRunner::default()).is_err());
}
