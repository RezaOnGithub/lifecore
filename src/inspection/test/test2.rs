use crate::{
    Body, EntryId, LifeCoreList, Payload, ViewError,
    analysis::AnalysisRunner,
    inspection::{
        Currentness, Direction, EdgeSelection, FieldFilter, NodeSelection, ResolutionDifference,
        TypeResolution,
    },
    transaction::{
        Change, EntryChange, NewEdge, NewNode,
        Reference::{Added, Existing},
        RevisionIntent, prepare,
    },
    typing::{FieldStatus, Type, TypeDefinition, ValueType},
};

fn node(title: &str, predecessor: Option<crate::transaction::Reference>) -> Change {
    Change::Entry(EntryChange::Node(NewNode {
        ty_name: "Note".into(),
        payload: Body::from([
            ("title".into(), title.into()),
            ("tags".into(), Payload::List(vec![])),
        ]),
        predecessor,
        intent: predecessor.map(|_| RevisionIntent::Branch),
    }))
}

fn fixture() -> (LifeCoreList, Vec<EntryId>) {
    let mut graph = LifeCoreList::default();
    let plan = prepare(
        &graph,
        &[
            node("A", None),
            node("B", None),
            Change::Entry(EntryChange::Edge(NewEdge {
                from: Added(0),
                ty_name: "link".into(),
                to: Added(1),
            })),
            node("A one", Some(Added(0))),
            node("A two", Some(Added(0))),
            Change::Entry(EntryChange::Edge(NewEdge {
                from: Added(3),
                ty_name: "link".into(),
                to: Added(1),
            })),
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
        assert_eq!(at.resolve(ids[4]).is_some(), end_exclusive >= 5);
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
        &[Change::Entry(EntryChange::Node(NewNode {
            ty_name: "Other".into(),
            payload: body,
            predecessor: None,
            intent: None,
        }))],
        &mut AnalysisRunner::default(),
    )
    .unwrap()
    .accepted()
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
            .map(|c| c.at_base().entry.id())
            .collect::<Vec<_>>(),
        vec![ids[0], ids[2]]
    );
    assert!(
        comparison
            .changed
            .iter()
            .all(|c| !c.at_base().stale && c.at_target().stale)
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
        &[Change::Entry(EntryChange::Node(NewNode {
            ty_name: "Type".into(),
            payload: ty.to_body(),
            predecessor: None,
            intent: None,
        }))],
        &mut AnalysisRunner::default(),
    )
    .unwrap()
    .accepted()
    .unwrap();
    let declaration = plan.added()[0];
    let comparison = graph.view().compare(plan.view()).unwrap();
    assert_eq!(comparison.changed.len(), 4);
    for change in comparison.changed {
        assert_eq!(
            change.at_base().type_definition,
            TypeResolution::MissingDeclaration
        );
        assert!(
            matches!(change.at_target().type_definition, TypeResolution::Resolved(TypeDefinition::Declared {id, ..}) if id == declaration)
        );
    }
    plan.commit(&mut graph).unwrap();
    let mut revised = ty;
    revised.fields.clear();
    let plan = prepare(
        &graph,
        &[Change::Entry(EntryChange::Node(NewNode {
            ty_name: "Type".into(),
            payload: revised.to_body(),
            predecessor: Some(Existing(declaration)),
            intent: Some(RevisionIntent::Branch),
        }))],
        &mut AnalysisRunner::default(),
    )
    .unwrap()
    .accepted()
    .unwrap();
    assert_eq!(graph.view().compare(plan.view()).unwrap().changed.len(), 5);
    assert_eq!(
        graph
            .view()
            .prefix(6)
            .unwrap()
            .resolve(ids[0])
            .unwrap()
            .type_definition,
        TypeResolution::MissingDeclaration
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
    .unwrap()
    .accepted()
    .unwrap();
    let two = prepare(
        &graph,
        &[node("candidate", None)],
        &mut AnalysisRunner::default(),
    )
    .unwrap()
    .accepted()
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
fn structured_inspection_is_deterministic_and_bounded() {
    let (mut graph, ids) = fixture();
    let plan = prepare(
        &graph,
        &[node("quote\" slash\\ line\n<angle>", None)],
        &mut AnalysisRunner::default(),
    )
    .unwrap()
    .accepted()
    .unwrap();
    plan.commit(&mut graph).unwrap();
    let before = graph.view().prefix(3).unwrap();
    let json = serde_json::to_value(before.inspection()).unwrap();
    assert_eq!(json["view"]["end_exclusive"], "3");
    assert_eq!(json["entries"].as_array().unwrap().len(), 3);
    assert!(!before.resolve(ids[2]).unwrap().stale);
    assert!(graph.view().resolve(ids[2]).unwrap().stale);
    let reload: LifeCoreList =
        serde_json::from_slice(&serde_json::to_vec(&graph).unwrap()).unwrap();
    assert_eq!(
        serde_json::to_vec(&graph.view().inspection()).unwrap(),
        serde_json::to_vec(&reload.view().inspection()).unwrap()
    );
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
                configuration: "".into()
            }],
            &mut AnalysisRunner::default()
        )
        .is_err()
    );
    assert_eq!(
        snapshot,
        serde_json::to_vec(&graph.view().inspection()).unwrap()
    );
    assert!(graph.view().resolve(ids[0]).is_some());
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
            *change.at_base(),
            base.resolve(change.at_base().entry.id()).unwrap()
        );
        assert_eq!(
            *change.at_target(),
            target.resolve(change.at_base().entry.id()).unwrap()
        );
        assert_eq!(change.at_base().entry, change.at_target().entry);
    }
    let same = base.compare(base).unwrap();
    assert!(same.added.is_empty());
    assert!(same.changed.is_empty());
    let empty = graph.view().prefix(0).unwrap();
    assert_eq!(empty.compare(base).unwrap().added.len(), 3);
    let mut encoded = serde_json::to_value(&graph).unwrap();
    encoded["entries"][0]["ty_name"] = serde_json::json!("Different");
    let divergent: LifeCoreList = serde_json::from_value(encoded).unwrap();
    assert_eq!(base.id(), divergent.view().prefix(3).unwrap().id());
    assert_eq!(
        base.compare(divergent.view()),
        Err(ViewError::DifferentHistory)
    );
}

#[test]
fn resolution_differences_require_changed_resolutions_of_the_same_entry() {
    let (graph, ids) = fixture();
    let base = graph.view().prefix(3).unwrap();
    let target = graph.view().prefix(4).unwrap();
    let original = base.resolve(ids[0]).unwrap();
    let revised = target.resolve(ids[0]).unwrap();
    assert!(ResolutionDifference::new(original.clone(), original.clone()).is_none());
    assert!(ResolutionDifference::new(revised.clone(), revised.clone()).is_none());
    assert!(ResolutionDifference::new(original.clone(), target.resolve(ids[1]).unwrap()).is_none());

    let mut edited_entry = original.entry.clone();
    let crate::Entry::Node(node) = &mut edited_entry else {
        unreachable!()
    };
    node.ty_name = "Altered payload with the same identity".into();
    let altered = super::super::Resolution {
        entry: &edited_entry,
        ..original.clone()
    };
    assert!(ResolutionDifference::new(original.clone(), altered).is_none());

    let difference = ResolutionDifference::new(original.clone(), revised.clone()).unwrap();
    assert_eq!(difference.at_base(), &original);
    assert_eq!(difference.at_target(), &revised);
    assert_eq!(difference.at_base().entry, difference.at_target().entry);
    assert_ne!(difference.at_base(), difference.at_target());
    let json = serde_json::to_value(&difference).unwrap();
    assert_eq!(json["at_base"]["stale"], false);
    assert_eq!(json["at_target"]["stale"], true);
}

#[test]
fn type_resolution_names_edge_missing_resolved_and_failed_outcomes_at_each_boundary() {
    use crate::typing::ResolutionError;
    let (mut graph, ids) = fixture();
    let undeclared = graph.view().resolve(ids[0]).unwrap().type_definition;
    let edge = graph.view().resolve(ids[2]).unwrap().type_definition;
    assert_eq!(undeclared, TypeResolution::MissingDeclaration);
    assert_eq!(edge, TypeResolution::NotApplicableToEdge);
    let schema = Type::new("Note", &[], &[], &[], &[]);
    let declaration = |schema: &Type| {
        Change::Entry(EntryChange::Node(NewNode {
            ty_name: "Type".into(),
            payload: schema.to_body(),
            predecessor: None,
            intent: None,
        }))
    };
    let checked = prepare(
        &graph,
        &[declaration(&schema)],
        &mut AnalysisRunner::default(),
    )
    .unwrap()
    .accepted()
    .unwrap();
    let schema_id = checked.added()[0];
    checked.commit(&mut graph).unwrap();
    let resolved = graph.view().resolve(ids[0]).unwrap().type_definition;
    assert_eq!(
        resolved,
        TypeResolution::Resolved(TypeDefinition::Declared {
            id: schema_id,
            schema: schema.clone()
        })
    );
    let builtin = graph.view().resolve(schema_id).unwrap().type_definition;
    assert_eq!(builtin, TypeResolution::Resolved(TypeDefinition::Builtin));
    assert_eq!(
        graph
            .view()
            .prefix(6)
            .unwrap()
            .resolve(ids[0])
            .unwrap()
            .type_definition,
        undeclared
    );

    let mut duplicate = schema;
    duplicate.outgoing.insert("link".into());
    let checked = prepare(
        &graph,
        &[declaration(&duplicate)],
        &mut AnalysisRunner::default(),
    )
    .unwrap()
    .accepted()
    .unwrap();
    let duplicate_id = checked.added()[0];
    checked.commit(&mut graph).unwrap();
    let ambiguous = graph.view().resolve(ids[0]).unwrap().type_definition;
    assert_eq!(
        ambiguous,
        TypeResolution::Failed(ResolutionError::Ambiguous {
            name: "Note".into(),
            entries: vec![schema_id, duplicate_id],
        })
    );
    assert_eq!(
        graph
            .view()
            .prefix(7)
            .unwrap()
            .resolve(ids[0])
            .unwrap()
            .type_definition,
        resolved
    );

    let checked = prepare(
        &graph,
        &[Change::Entry(EntryChange::Node(NewNode {
            ty_name: "Type".into(),
            payload: Body::new(),
            predecessor: None,
            intent: None,
        }))],
        &mut AnalysisRunner::default(),
    )
    .unwrap()
    .accepted()
    .unwrap();
    let malformed_id = checked.added()[0];
    checked.commit(&mut graph).unwrap();
    let malformed = graph.view().resolve(ids[0]).unwrap().type_definition;
    assert!(
        matches!(&malformed, TypeResolution::Failed(ResolutionError::Malformed { id, .. }) if *id == malformed_id)
    );
    assert_eq!(
        graph
            .view()
            .prefix(8)
            .unwrap()
            .resolve(ids[0])
            .unwrap()
            .type_definition,
        ambiguous
    );
    assert_eq!(graph.view().resolve(ids[2]).unwrap().type_definition, edge);
    for resolution in [undeclared, edge, resolved, builtin, ambiguous, malformed] {
        let encoded = serde_json::to_vec(&resolution).unwrap();
        let decoded: TypeResolution = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, resolution);
    }
}
