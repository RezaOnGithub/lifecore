use super::super::{Change, Error, Store, check};
use crate::{Body, EntryIndex, LifeCore, LifeCoreAccess, LifeCoreList, analysis::AnalysisRunner};

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
            from: EntryIndex(0),
            ty_name: "link".into(),
            to: EntryIndex(1),
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
    assert!(graph.last().is_none());
    assert_eq!(checked.graph().last().unwrap().index(), EntryIndex(2));
    checked.commit(&mut graph).unwrap();
    assert!(!graph.is_mock());
    assert_eq!(graph.last().unwrap().index(), EntryIndex(2));
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
    assert!(graph.last().is_none());
}

#[test]
fn storage_round_trip_no_clobber_stale_base_and_failed_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("graph.json");
    let mut store = Store::new(&path);
    let graph = LifeCoreList::default();
    let mut runner = AnalysisRunner::default();
    store.initialize(&graph).unwrap();
    let before = std::fs::read(&path).unwrap();
    assert!(store.initialize(&graph).is_err());
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
    assert!(loaded.edge(EntryIndex(2).before_and_including()).is_some());
}

#[test]
fn stale_memory_candidate_is_rejected() {
    let mut graph = LifeCoreList::default();
    let checked = check(&graph, &plan(), &mut AnalysisRunner::default()).unwrap();
    graph.add_mock("Other", Body::default(), None).unwrap();
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
    encoded["entry"][2]["Edge"]["from"] = serde_json::json!(9999);
    std::fs::write(&path, serde_json::to_vec(&encoded).unwrap()).unwrap();
    assert!(Store::new(path).load().is_err());
}
