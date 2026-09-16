use crate::{
    Body, EntryId, LifeCoreList, Payload, ViewError,
    analysis::AnalysisRunner,
    inspection::{Currentness, Direction, EdgeSelection, FieldFilter, NodeSelection},
    transaction::{
        Change,
        Reference::{Added, Existing},
        prepare,
    },
    typing::{FieldStatus, Type, TypeDefinition, ValueType},
};

fn node(title: &str, parent: Option<crate::transaction::Reference>) -> Change {
    Change::AddNode {
        ty_name: "Note".into(),
        body: Body::from([
            ("title".into(), title.into()),
            ("tags".into(), Payload::List(vec![])),
        ]),
        parent,
    }
}

fn fixture() -> (LifeCoreList, Vec<EntryId>) {
    let mut graph = LifeCoreList::default();
    let plan = prepare(
        &graph,
        &[
            node("A", None),
            node("B", None),
            Change::AddEdge {
                from: Added(0),
                ty_name: "link".into(),
                to: Added(1),
            },
            node("A one", Some(Added(0))),
            node("A two", Some(Added(0))),
            Change::AddEdge {
                from: Added(3),
                ty_name: "link".into(),
                to: Added(1),
            },
        ],
        &mut AnalysisRunner::default(),
    )
    .unwrap();
    let ids = plan.added().to_vec();
    plan.commit(&mut graph).unwrap();
    (graph, ids)
}

#[test]
fn selection_and_exact_relationships_follow_each_split_boundary() {
    let (graph, ids) = fixture();
    let view = graph.view();
    for (end_exclusive, current, historical, descendants) in [
        (3, vec![ids[0], ids[1]], vec![], vec![]),
        (4, vec![ids[1], ids[3]], vec![ids[0]], vec![ids[3]]),
        (
            5,
            vec![ids[1], ids[3], ids[4]],
            vec![ids[0]],
            vec![ids[3], ids[4]],
        ),
        (
            6,
            vec![ids[1], ids[3], ids[4]],
            vec![ids[0]],
            vec![ids[3], ids[4]],
        ),
    ] {
        let at = view.prefix(end_exclusive).unwrap();
        assert_eq!(
            at.select_nodes(&NodeSelection::default())
                .map(|n| n.id)
                .collect::<Vec<_>>(),
            current
        );
        assert_eq!(
            at.select_nodes(&NodeSelection {
                currentness: Currentness::Historical,
                ..Default::default()
            })
            .map(|n| n.id)
            .collect::<Vec<_>>(),
            historical
        );
        assert_eq!(
            at.descendants(ids[0]).map(|n| n.id).collect::<Vec<_>>(),
            descendants
        );
        let incoming = at
            .relationships(ids[1], Direction::Incoming, EdgeSelection::default())
            .map(|e| e.id)
            .collect::<Vec<_>>();
        assert_eq!(
            incoming,
            match end_exclusive {
            3 => vec![ids[2]],
            6 => vec![ids[5]],
            _ => vec![],
            }
        );
        assert_eq!(
            at.relationships(
                ids[0],
                Direction::Outgoing,
                EdgeSelection {
                    currentness: Currentness::All,
                    ty_name: Some("link")
                }
            )
            .map(|e| e.to)
            .collect::<Vec<_>>(),
            vec![ids[1]]
        );
        assert_eq!(at.inspect(ids[4]).is_some(), end_exclusive >= 5);
    }
    assert!(view.prefix(3).unwrap().descendants(ids[3]).next().is_none());
    assert!(
        view.relationships(ids[1], Direction::Outgoing, EdgeSelection::default())
            .next()
            .is_none()
    );
    assert!(
        view.select_edges(EdgeSelection {
            ty_name: Some("absent"),
            ..Default::default()
        })
        .next()
        .is_none()
    );
}

#[test]
fn field_presence_equality_and_recursive_text_are_literal_and_composable() {
    let (mut graph, ids) = fixture();
    let body = Body::from([(
        "rows".into(),
        Payload::List(vec![Payload::Record(Body::from([(
            "key".into(),
            "Deep needle".into(),
        )]))]),
    )]);
    let plan = prepare(
        &graph,
        &[Change::AddNode {
            ty_name: "Other".into(),
            body,
            parent: None,
        }],
        &mut AnalysisRunner::default(),
    )
    .unwrap();
    let nested = plan.added()[0];
    plan.commit(&mut graph).unwrap();
    let empty = Payload::List(vec![]);
    let title = Payload::from("A two");
    let selection = NodeSelection {
        ty_name: Some("Note"),
        fields: vec![
            FieldFilter::Present("tags"),
            FieldFilter::Equal("tags", &empty),
            FieldFilter::Equal("title", &title),
            FieldFilter::Absent("missing"),
        ],
        text: Some("two"),
        ..Default::default()
    };
    assert_eq!(
        graph
            .view()
            .select_nodes(&selection)
            .map(|n| n.id)
            .collect::<Vec<_>>(),
        vec![ids[4]]
    );
    assert_eq!(
        graph
            .view()
            .select_nodes(&NodeSelection {
                text: Some("needle"),
                ..Default::default()
            })
            .map(|n| n.id)
            .collect::<Vec<_>>(),
        vec![nested]
    );
    for text in ["Needle", "rows", "Other"] {
        assert!(
            graph
                .view()
                .select_nodes(&NodeSelection {
                    text: Some(text),
                    ..Default::default()
                })
                .next()
                .is_none()
        );
    }
}

#[test]
fn comparisons_show_edge_currentness_and_schema_reinterpretation() {
    let (mut graph, ids) = fixture();
    let before = graph.view().prefix(3).unwrap();
    let middle = graph.view().prefix(4).unwrap();
    let comparison = before.compare(middle).unwrap();
    assert_eq!(comparison.added[0].entry.id(), ids[3]);
    assert_eq!(
        comparison
            .changed
            .iter()
            .map(|c| c.at_base.entry.id())
            .collect::<Vec<_>>(),
        vec![ids[0], ids[2]]
    );
    assert!(
        comparison
            .changed
            .iter()
            .all(|c| !c.at_base.stale && c.at_target.stale)
    );
    assert!(middle.compare(before).is_err());
    let ty = Type::new(
        "Note",
        &[("title", (ValueType::Text, FieldStatus::Optional))],
        &[],
        &[],
        &[],
    );
    let plan = prepare(
        &graph,
        &[Change::AddNode {
            ty_name: "Type".into(),
            body: ty.to_body(),
            parent: None,
        }],
        &mut AnalysisRunner::default(),
    )
    .unwrap();
    let declaration = plan.added()[0];
    let comparison = graph.view().compare(plan.view()).unwrap();
    assert_eq!(comparison.changed.len(), 4);
    for change in comparison.changed {
        assert_eq!(change.at_base.type_definition, Some(Ok(None)));
        assert!(
            matches!(change.at_target.type_definition, Some(Ok(Some(TypeDefinition::Declared {id, ..}))) if id == declaration)
        );
    }
    plan.commit(&mut graph).unwrap();
    let mut revised = ty;
    revised.body.clear();
    let plan = prepare(
        &graph,
        &[Change::AddNode {
            ty_name: "Type".into(),
            body: revised.to_body(),
            parent: Some(Existing(declaration)),
        }],
        &mut AnalysisRunner::default(),
    )
    .unwrap();
    assert_eq!(graph.view().compare(plan.view()).unwrap().changed.len(), 5);
    assert_eq!(
        graph
            .view()
            .prefix(6)
            .unwrap()
            .inspect(ids[0])
            .unwrap()
            .type_definition,
        Some(Ok(None))
    );
}

#[test]
fn comparison_rejects_foreign_histories_and_sibling_candidates() {
    let (graph, _) = fixture();
    let one = prepare(
        &graph,
        &[node("candidate", None)],
        &mut AnalysisRunner::default(),
    )
    .unwrap();
    let two = prepare(
        &graph,
        &[node("candidate", None)],
        &mut AnalysisRunner::default(),
    )
    .unwrap();
    assert!(graph.view().compare(one.view()).is_ok());
    assert_eq!(
        one.view().compare(two.view()),
        Err(ViewError::DifferentHistory)
    );
    assert_eq!(
        graph.view().compare(LifeCoreList::default().view()),
        Err(ViewError::DifferentHistory)
    );
    let reloaded: LifeCoreList =
        serde_json::from_slice(&serde_json::to_vec(&graph).unwrap()).unwrap();
    assert!(
        graph
            .view()
            .compare(reloaded.view())
            .unwrap()
            .changed
            .is_empty()
    );
}

#[test]
fn structured_inspection_and_dot_are_deterministic_bounded_and_escape_labels() {
    let (mut graph, ids) = fixture();
    let plan = prepare(
        &graph,
        &[node("quote\" slash\\ line\n<angle>", None)],
        &mut AnalysisRunner::default(),
    )
    .unwrap();
    plan.commit(&mut graph).unwrap();
    let before = graph.view().prefix(3).unwrap();
    let dot = before.to_dot();
    assert!(!dot.contains("revision"));
    assert!(!dot.contains("n3"));
    let dot = graph.view().to_dot();
    assert!(dot.contains("n0 -> n3 [label=\"revision\", style=dashed, color=blue]"));
    assert!(dot.contains("n0 -> n1 [label=\"@2 link\", style=solid, color=gray]"));
    assert!(dot.contains("n3 -> n1 [label=\"@5 link\", style=solid, color=black]"));
    assert!(dot.contains("quote\\\\\\\" slash\\\\\\\\ line\\\\n<angle>"));
    assert_eq!(dot, graph.view().to_dot());
    let json = serde_json::to_value(before.inspection()).unwrap();
    assert_eq!(json["view"]["end_exclusive"], 3);
    assert_eq!(json["entries"].as_array().unwrap().len(), 3);
    assert!(!before.inspect(ids[2]).unwrap().stale);
    assert!(graph.view().inspect(ids[2]).unwrap().stale);
    let reload: LifeCoreList =
        serde_json::from_slice(&serde_json::to_vec(&graph).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_vec(&graph.view().inspection()).unwrap(),
        serde_json::to_vec(&reload.view().inspection()).unwrap()
    );
    assert_eq!(dot, reload.view().to_dot());
}

#[test]
fn failed_drafts_and_missing_analyzers_leave_committed_inspection_available() {
    let (graph, ids) = fixture();
    let snapshot = serde_json::to_vec(&graph.view().inspection()).unwrap();
    assert!(
        prepare(
            &graph,
            &[node("bad", Some(Added(0)))],
            &mut AnalysisRunner::default()
        )
        .is_err()
    );
    assert!(
        prepare(
            &graph,
            &[Change::AddAnalyzer {
                name: "unavailable".into(),
                config: "".into()
            }],
            &mut AnalysisRunner::default()
        )
        .is_err()
    );
    assert_eq!(
        snapshot,
        serde_json::to_vec(&graph.view().inspection()).unwrap()
    );
    assert!(graph.view().inspect(ids[0]).is_some());
    assert!(
        LifeCoreList::default()
            .view()
            .inspection()
            .entries
            .is_empty()
    );
}

#[test]
fn comparisons_use_exclusive_boundaries_and_retain_both_inspected_entries() {
    let (graph, ids) = fixture();
    let base = graph.view().prefix(3).unwrap();
    let target = graph.view().prefix(5).unwrap();
    let comparison = base.compare(target).unwrap();
    assert_eq!(comparison.base_view, base.id());
    assert_eq!(comparison.target_view, target.id());
    assert_eq!(comparison.base_view.end_exclusive, 3);
    assert_eq!(comparison.target_view.end_exclusive, 5);
    assert_eq!(
        comparison
            .added
            .iter()
            .map(|entry| entry.entry.id())
            .collect::<Vec<_>>(),
        ids[3..5]
    );
    for change in &comparison.changed {
        assert_eq!(
            change.at_base,
            base.inspect(change.at_base.entry.id()).unwrap()
        );
        assert_eq!(
            change.at_target,
            target.inspect(change.at_base.entry.id()).unwrap()
        );
        assert_eq!(change.at_base.entry, change.at_target.entry);
    }
    let same = base.compare(base).unwrap();
    assert!(same.added.is_empty());
    assert!(same.changed.is_empty());
    let empty = graph.view().prefix(0).unwrap();
    assert_eq!(empty.compare(base).unwrap().added.len(), 3);
    let mut encoded = serde_json::to_value(&graph).unwrap();
    encoded["entries"][0]["Node"]["ty_name"] = serde_json::json!("Different");
    let divergent: LifeCoreList = serde_json::from_value(encoded).unwrap();
    assert_eq!(base.id(), divergent.view().prefix(3).unwrap().id());
    assert_eq!(
        base.compare(divergent.view()),
        Err(ViewError::DifferentHistory)
    );
}
