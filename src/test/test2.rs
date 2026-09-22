use crate::{
    Body, EntryId, LifeCoreList, Payload, ViewError,
    analysis::AnalysisRunner,
    transaction::{
        Change,
        Reference::{Added, Existing},
        prepare,
    },
    typing::{
        FieldDecl, FieldIssueKind, FieldStatus, PathPart, SchemaErrorKind, Type, TypeDefinition,
        ValueType,
    },
};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

fn node(name: &str, parent: Option<crate::transaction::Reference>) -> Change {
    Change::AddNode {
        ty_name: name.into(),
        body: Body::default(),
        parent,
    }
}
fn append(graph: &mut LifeCoreList, changes: &[Change]) -> Vec<EntryId> {
    let plan = prepare(graph, changes, &mut AnalysisRunner::default()).unwrap();
    let ids = plan.added().to_vec();
    plan.commit(graph).unwrap();
    ids
}
fn hash(value: &impl Hash) -> u64 {
    let mut state = DefaultHasher::new();
    value.hash(&mut state);
    state.finish()
}

#[test]
fn empty_view_has_builtin_type_and_cannot_expand() {
    let graph = LifeCoreList::default();
    let view = graph.view();
    assert!(view.is_empty());
    assert!(view.at(0).is_none());
    assert_eq!(
        view.resolve_type("Type").unwrap(),
        Some(TypeDefinition::Builtin)
    );
    assert!(matches!(
        view.prefix(1),
        Err(ViewError::BeyondBoundary { .. })
    ));
}

#[test]
fn revision_split_and_exact_edges_are_interpreted_in_each_view() {
    let mut graph = LifeCoreList::default();
    let ids = append(
        &mut graph,
        &[
            node("A", None),
            node("B", None),
            Change::AddEdge {
                from: Added(0),
                ty_name: "link".into(),
                to: Added(1),
            },
            node("A2", Some(Added(0))),
            node("A3", Some(Added(0))),
            Change::AddEdge {
                from: Added(3),
                ty_name: "link".into(),
                to: Added(1),
            },
        ],
    );
    let view = graph.view();
    let before = view.prefix(3).unwrap();
    assert_eq!(before.is_stale(ids[0]), Some(false));
    assert_eq!(before.edges_of(ids[0]).count(), 1);
    assert_eq!(before.fresh_edges().count(), 1);
    assert!(before.get(ids[3]).is_none());
    assert!(before.prefix(4).is_err());
    let middle = view.prefix(4).unwrap();
    assert_eq!(middle.is_stale(ids[2]), Some(true));
    assert_eq!(middle.fresh_edges().count(), 0);
    assert_eq!(middle.node(ids[3]).unwrap().parent, Some(ids[0]));
    assert_eq!(
        view.fresh_successors(ids[0])
            .map(|n| n.id)
            .collect::<Vec<_>>(),
        vec![ids[3], ids[4]]
    );
    assert_eq!(view.relative_degree(ids[3], ids[4]), Some(0));
    assert_eq!(view.relative_degree(ids[0], ids[3]), None);
    assert_eq!(view.root_of(ids[4]), Some(ids[0]));
    assert_eq!(
        view.fresh_edges().map(|e| e.id).collect::<Vec<_>>(),
        vec![ids[5]]
    );
    assert_eq!(view.edge(ids[2]).unwrap().from, ids[0]);
    assert!(view.extends(before).is_ok());
}

#[test]
fn ancestry_excludes_self_and_preserves_roots_and_current_descendants_in_each_prefix() {
    let mut graph = LifeCoreList::default();
    let ids = append(
        &mut graph,
        &[
            node("A", None),
            node("B", None),
            Change::AddEdge {
                from: Added(0),
                ty_name: "link".into(),
                to: Added(1),
            },
            node("Child", Some(Added(0))),
            node("Grandchild", Some(Added(3))),
            node("Branch", Some(Added(0))),
        ],
    );
    let mut other = LifeCoreList::default();
    let foreign = append(&mut other, &[node("A", None)])[0];
    for (end_exclusive, descendants, current) in [
        (0, vec![], vec![]),
        (1, vec![], vec![ids[0]]),
        (2, vec![], vec![ids[0]]),
        (3, vec![], vec![ids[0]]),
        (4, vec![ids[3]], vec![ids[3]]),
        (5, vec![ids[3], ids[4]], vec![ids[4]]),
        (6, vec![ids[3], ids[4], ids[5]], vec![ids[4], ids[5]]),
    ] {
        let view = graph.view().prefix(end_exclusive).unwrap();
        for (id, ancestors, root) in [
            (ids[0], vec![], ids[0]),
            (ids[1], vec![], ids[1]),
            (ids[3], vec![ids[0]], ids[0]),
            (ids[4], vec![ids[3], ids[0]], ids[0]),
            (ids[5], vec![ids[0]], ids[0]),
        ] {
            let visible = id.position() < end_exclusive;
            assert_eq!(
                view.ancestors(id).map(|n| n.id).collect::<Vec<_>>(),
                if visible { ancestors } else { vec![] }
            );
            assert_eq!(view.root_of(id), visible.then_some(root));
        }
        assert_eq!(
            view.descendants(ids[0]).map(|n| n.id).collect::<Vec<_>>(),
            descendants
        );
        assert_eq!(
            view.fresh_successors(ids[0])
                .map(|n| n.id)
                .collect::<Vec<_>>(),
            current
        );
        for id in [foreign, ids[2]] {
            assert!(view.ancestors(id).next().is_none());
            assert_eq!(view.root_of(id), None);
            assert!(view.descendants(id).next().is_none());
            assert!(view.fresh_successors(id).next().is_none());
        }
    }
    let view = graph.view();
    assert_eq!(
        view.fresh_successors(ids[4])
            .map(|n| n.id)
            .collect::<Vec<_>>(),
        vec![ids[4]]
    );
    assert!(view.descendants(ids[4]).next().is_none());
    assert_eq!(view.relative_degree(ids[3], ids[5]), Some(0));
    assert_eq!(view.relative_degree(ids[4], ids[5]), Some(1));
    assert_eq!(view.relative_degree(ids[5], ids[4]), Some(1));
    assert_eq!(view.relative_degree(ids[4], ids[1]), None);
}

#[test]
fn identities_reject_other_databases_and_independent_candidate_suffixes() {
    let mut graph = LifeCoreList::default();
    let base = append(&mut graph, &[node("A", None)])[0];
    let first = prepare(&graph, &[node("B", None)], &mut AnalysisRunner::default()).unwrap();
    let second = prepare(&graph, &[node("B", None)], &mut AnalysisRunner::default()).unwrap();
    let a = first.added()[0];
    let b = second.added()[0];
    assert_eq!(a.position(), b.position());
    assert_ne!(a, b);
    assert!(first.view().node(base).is_some());
    assert!(second.view().get(a).is_none());
    assert_ne!(first.view().id(), second.view().id());
    assert_eq!(first.view().common_prefix(second.view()).unwrap(), 1);
    assert!(first.view().extends(second.view()).is_err());
    let mut other = LifeCoreList::default();
    let foreign = append(&mut other, &[node("A", None)])[0];
    assert!(graph.view().get(foreign).is_none());
    assert!(graph.view().common_prefix(other.view()).is_err());
    assert!(
        prepare(
            &graph,
            &[node("Child", Some(Existing(foreign)))],
            &mut AnalysisRunner::default()
        )
        .is_err()
    );
}

#[test]
fn duplicate_and_invalid_reference_rules_leave_the_published_graph_unchanged() {
    let graph = LifeCoreList::default();
    let cases = [
        vec![node("A", None), node("A", None)],
        vec![
            node("A", None),
            Change::AddEdge {
                from: Added(0),
                ty_name: "x".into(),
                to: Added(0),
            },
        ],
        vec![node("A", Some(Added(0)))],
        vec![
            node("A", None),
            node("B", None),
            Change::AddEdge {
                from: Added(0),
                ty_name: "x".into(),
                to: Added(1),
            },
            node("Child", Some(Added(2))),
        ],
        vec![
            node("A", None),
            node("B", None),
            node("A2", Some(Added(0))),
            Change::AddEdge {
                from: Added(0),
                ty_name: "x".into(),
                to: Added(1),
            },
        ],
        vec![
            node("A", None),
            node("B", None),
            Change::AddEdge {
                from: Added(0),
                ty_name: "x".into(),
                to: Added(1),
            },
            Change::AddEdge {
                from: Added(0),
                ty_name: "x".into(),
                to: Added(1),
            },
        ],
    ];
    for changes in cases {
        assert!(prepare(&graph, &changes, &mut AnalysisRunner::default()).is_err());
        assert!(graph.view().is_empty());
    }
}

#[test]
fn stale_duplicate_nodes_and_branching_from_old_parents_are_permitted() {
    let mut graph = LifeCoreList::default();
    let ids = append(
        &mut graph,
        &[node("A", None), node("A", Some(Added(0))), node("A", None)],
    );
    assert_eq!(graph.view().fresh_nodes().count(), 2);
    append(&mut graph, &[node("Branch", Some(Existing(ids[0])))]);
    assert_eq!(graph.view().fresh_successors(ids[0]).count(), 2);
}

#[test]
fn entry_identity_ordering_traits_agree() {
    let graph = LifeCoreList::default();
    let candidate = prepare(
        &graph,
        &[node("A", None), node("B", None)],
        &mut AnalysisRunner::default(),
    )
    .unwrap();
    let [a, b] = candidate.added() else {
        unreachable!()
    };
    assert_eq!(a.partial_cmp(b), Some(a.cmp(b)));
}

#[test]
fn records_hash_independently_of_insertion_order_and_lists_retain_order() {
    let a = Payload::Record(Body::from([
        ("a".into(), 1.into()),
        ("b".into(), true.into()),
    ]));
    let b = Payload::Record(Body::from([
        ("b".into(), true.into()),
        ("a".into(), 1.into()),
    ]));
    assert_eq!(a, b);
    assert_eq!(hash(&a), hash(&b));
    assert_eq!(
        serde_json::to_vec(&a).unwrap(),
        serde_json::to_vec(&b).unwrap()
    );
    assert_ne!(
        Payload::List(vec![1.into(), 2.into()]),
        Payload::List(vec![2.into(), 1.into()])
    );
    assert_ne!(Payload::Number(1), Payload::Boolean(true));
}

#[test]
fn scalar_and_nested_payloads_round_trip_without_coercion() {
    let time: crate::Timestamp = "2026-09-14T12:00:00Z".parse().unwrap();
    let value = Payload::Record(Body::from([(
        "values".into(),
        Payload::List(vec![
            "text".into(),
            1.into(),
            true.into(),
            time.into(),
            Payload::Record(Body::new()),
        ]),
    )]));
    assert_eq!(
        serde_json::from_slice::<Payload>(&serde_json::to_vec(&value).unwrap()).unwrap(),
        value
    );
    assert!(serde_json::from_str::<Payload>(r#"{"kind":"number","value":1.5}"#).is_err());
    assert!(serde_json::from_str::<Payload>(r#"{"kind":"float","value":1.0}"#).is_err());
}

#[test]
fn schemas_round_trip_as_named_records_and_nested_shapes_report_precise_paths() {
    let record = ValueType::Record(
        [(
            "label".into(),
            FieldDecl {
                ty: ValueType::Text,
                status: FieldStatus::Mandatory,
            },
        )]
        .into(),
    );
    let ty = Type::new(
        "Thing",
        &[(
            "rows",
            (ValueType::List(Box::new(record)), FieldStatus::Mandatory),
        )],
        &["link"],
        &[],
        &[],
    );
    assert_eq!(Type::from_body(&ty.to_body()).unwrap(), ty);
    assert!(
        ty.check(&Body::from([("rows".into(), Payload::List(vec![]))]))
            .is_empty()
    );
    let issues = ty.check(&Body::from([(
        "rows".into(),
        Payload::List(vec![Payload::Record(Body::new())]),
    )]));
    assert_eq!(issues[0].kind, FieldIssueKind::MissingMandatory);
    assert_eq!(
        issues[0].path,
        vec![
            PathPart::Field("rows".into()),
            PathPart::Index(0),
            PathPart::Field("label".into())
        ]
    );
    let mut malformed = ty.to_body();
    malformed.insert("typo".into(), true.into());
    assert_eq!(
        Type::from_body(&malformed).unwrap_err().kind,
        SchemaErrorKind::UnknownField
    );
}

#[test]
fn deserialization_rejects_cyclic_forward_and_mismatched_identities_before_inspection() {
    let mut graph = LifeCoreList::default();
    append(&mut graph, &[node("A", None), node("B", Some(Added(0)))]);
    let original = serde_json::to_value(&graph).unwrap();
    let mut cyclic = original.clone();
    cyclic["entries"][0]["Node"]["parent"] = cyclic["entries"][0]["Node"]["id"].clone();
    assert!(serde_json::from_value::<LifeCoreList>(cyclic).is_err());
    let mut forward = original.clone();
    forward["entries"][0]["Node"]["parent"] = forward["entries"][1]["Node"]["id"].clone();
    assert!(serde_json::from_value::<LifeCoreList>(forward).is_err());
    let mut wrong = original;
    wrong["entries"][1]["Node"]["id"]["position"] = 99.into();
    assert!(serde_json::from_value::<LifeCoreList>(wrong).is_err());
}

#[test]
fn prefix_inspection_matches_the_same_stored_history_truncated_at_that_boundary() {
    let mut full = LifeCoreList::default();
    let ids = append(
        &mut full,
        &[
            node("A", None),
            node("B", None),
            node("A2", Some(Added(0))),
            node("A3", Some(Added(0))),
        ],
    );
    for end_exclusive in 0..=full.view().len() {
        let mut truncated = full.clone();
        truncated.entries.truncate(end_exclusive);
        let bounded = full.view().prefix(end_exclusive).unwrap();
        assert_eq!(bounded.id(), truncated.view().id());
        for id in &ids {
            assert_eq!(bounded.get(*id), truncated.view().get(*id));
            assert_eq!(bounded.is_stale(*id), truncated.view().is_stale(*id));
        }
        assert_eq!(
            bounded.fresh_nodes().collect::<Vec<_>>(),
            truncated.view().fresh_nodes().collect::<Vec<_>>()
        );
        assert_eq!(
            bounded.fresh_successors(ids[0]).collect::<Vec<_>>(),
            truncated
                .view()
                .fresh_successors(ids[0])
                .collect::<Vec<_>>()
        );
    }
}
