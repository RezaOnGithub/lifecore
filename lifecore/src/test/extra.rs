//! Tests written by Agents

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use crate::{
    EdgeExt, LifeCore, LifeCoreList, NodeBodyExt, NodeExt, PointedEventContext,
    analysis::{Suggestion, Warning},
};

use super::*;

fn connected_graph() -> (LifeCoreList, NodeExt, NodeExt, EdgeExt) {
    let mut lc = LifeCoreList::default();
    let from = lc.add("From", NodeBodyExt::default(), None).unwrap();
    let to = lc.add("To", NodeBodyExt::default(), None).unwrap();
    let edge = lc.connect(from.e, "link", to.e).unwrap();
    (lc, from, to, edge)
}

fn hash<T: Hash>(value: &T) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[test]
fn fresh_edges_of_finds_an_edge_from_the_current_context() {
    let (lc, from, _, edge) = connected_graph();

    let edges = lc.fresh_edges_of(from.e.absolute()).collect::<Vec<_>>();

    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].e.absolute(), edge.e.absolute());
}

#[test]
fn fresh_outgoing_edges_of_finds_an_edge_from_the_current_context() {
    let (lc, from, _, edge) = connected_graph();

    let edges = lc
        .fresh_outgoing_edges_of(from.e.absolute())
        .collect::<Vec<_>>();

    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].e.absolute(), edge.e.absolute());
}

#[test]
fn fresh_incoming_edges_to_finds_an_edge_from_the_current_context() {
    let (lc, _, to, edge) = connected_graph();

    let edges = lc
        .fresh_incoming_edges_to(to.e.absolute())
        .collect::<Vec<_>>();

    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].e.absolute(), edge.e.absolute());
}

#[test]
fn fresh_edges_exclude_an_edge_with_a_stale_endpoint() {
    let (mut lc, from, to, _) = connected_graph();
    lc.add("To", NodeBodyExt::default(), Some(to.e)).unwrap();

    assert_eq!(lc.fresh_edges_of(from.e.absolute()).count(), 0);
}

// TODO revise
#[cfg(never)]
#[test]
fn families_can_include_a_root_from_before_the_search_window() {
    // let mut lc = LifeCoreList::new();
    // let root = lc.add("Root", NodeBodyExt::default(), None).unwrap();
    // let child = lc
    //     .add("Child", NodeBodyExt::default(), Some(root.e))
    //     .unwrap();
    // let child_only_window = root.e.absolute().exclusive_right_context();

    // let families = lc.non_solo_families(child_only_window);

    // assert_eq!(families.len(), 1);
    // assert_eq!(families[0].len(), 2);
    // assert!(
    //     families[0]
    //         .iter()
    //         .any(|event| event.absolute() == root.e.absolute())
    // );
    // assert!(
    //     families[0]
    //         .iter()
    //         .any(|event| event.absolute() == child.e.absolute())
    // );
}

// TODO revise
// Hash of Message needs more testing
#[test]
fn warning_hash_is_independent_of_relevance_order_for_the_same_event() {
    let wide = PointedEventContext { end: 4, thing: 1 };
    let narrow = PointedEventContext { end: 3, thing: 1 };
    let warning = |relevance| Warning {
        offending_event: wide.into(),
        system_name: "test".to_string(),
        system_config: String::new(),
        relevance,
        message: "same warning".to_string(),
    };

    let forward = warning(vec![wide, narrow]);
    let reverse = warning(vec![narrow, wide]);

    assert_eq!(hash(&forward), hash(&reverse));
}

#[test]
fn suggestion_hash_is_independent_of_relevance_order_for_the_same_event() {
    let wide = PointedEventContext { end: 4, thing: 1 };
    let narrow = PointedEventContext { end: 3, thing: 1 };
    let suggestion = |relevance| Suggestion {
        offending_event: wide.into(),
        system_name: "test".to_string(),
        system_config: String::new(),
        relevance,
        message: "same suggestion".to_string(),
        action: vec![],
    };

    let forward = suggestion(vec![wide, narrow]);
    let reverse = suggestion(vec![narrow, wide]);

    assert_eq!(hash(&forward), hash(&reverse));
}
