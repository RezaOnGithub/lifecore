//! Tests written and maintained by Agents

use crate::{FieldQuery, FieldQueryPathComponent};

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use crate::{
    Edge, LifeCore, LifeCoreList, Body, Node, EntryContext,
    analysis::{Suggestion, Warning},
};

use super::*;

fn connected_graph() -> (LifeCoreList, Node, Node, Edge) {
    let mut lc = LifeCoreList::default();
    let from = lc.add_mock("From", Body::default(), None).unwrap();
    let to = lc.add_mock("To", Body::default(), None).unwrap();
    let edge = lc.connect_mock(from.e, "link", to.e).unwrap();
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
    lc.add_mock("To", Body::default(), Some(to.e)).unwrap();

    assert_eq!(lc.fresh_edges_of(from.e.absolute()).count(), 0);
}

#[test]
fn node_parent_inherits_the_inspection_window() {
    let mut lc = LifeCoreList::default();
    let root = lc.add_mock("Root", Body::default(), None).unwrap();
    let child = lc
        .add_mock("Child", Body::default(), Some(root.e))
        .unwrap();
    let _later = lc.add_mock("Later", Body::default(), None).unwrap();
    let child_at_end = lc.last().unwrap().look_at(child.e);

    let child = lc.node(child_at_end).unwrap();

    assert_eq!(child.parent, Some(root.e.entry()));
}

// TODO revise
#[cfg(never)]
#[test]
fn families_can_include_a_root_from_before_the_search_window() {
    // let mut lc = LifeCoreList::new();
    // let root = lc.add_mock("Root", NodeBodyExt::default(), None).unwrap();
    // let child = lc
    //     .add_mock("Child", NodeBodyExt::default(), Some(root.e))
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

#[cfg(revise)]
#[test]
fn warning_hash_is_independent_of_relevance_order_for_the_same_event() {
    let wide = EntryContext { end: 4, thing: 1 };
    let narrow = EntryContext { end: 3, thing: 1 };
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

#[cfg(revise)]
#[test]
fn suggestion_hash_is_independent_of_relevance_order_for_the_same_event() {
    let wide = EntryContext { end: 4, thing: 1 };
    let narrow = EntryContext { end: 3, thing: 1 };
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

#[test]
fn field_query_parser_and_display_round_trip() {
    let cases = [
        ("title", "title"),
        ("  items [ 12 ] [ * ] [ last ]  ", "items[12][*][Last]"),
        ("matrix[0][1][Last]", "matrix[0][1][Last]"),
    ];

    for (input, canonical) in cases {
        let parsed = input.parse::<FieldQuery>().unwrap();
        assert_eq!(parsed.to_string(), canonical);

        let reparsed = parsed.to_string().parse::<FieldQuery>().unwrap();
        assert_eq!(reparsed.field_name, parsed.field_name);
        assert!(reparsed.path == parsed.path);
    }
}

#[test]
fn field_query_parser_preserves_selector_kinds() {
    let parsed = "values[0][*][last]".parse::<FieldQuery>().unwrap();

    assert_eq!(parsed.field_name, "values");
    assert!(matches!(
        parsed.path.as_slice(),
        [
            FieldQueryPathComponent::Index(0),
            FieldQueryPathComponent::Wildcard,
            FieldQueryPathComponent::Last,
        ]
    ));
}

#[test]
fn field_query_parser_rejects_bad_names_keywords_and_selectors() {
    for input in [
        "",
        "Items[0]",
        "itemName[0]",
        "_items[0]",
        "items[]",
        "items[-1]",
        "items[LAST]",
        "items[0]tail",
    ] {
        assert!(
            input.parse::<FieldQuery>().is_err(),
            "{input:?} unexpectedly parsed"
        );
    }
}
