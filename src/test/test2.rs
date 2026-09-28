use crate::{
    Body, EntryId, LifeCoreList, Payload, ViewError,
    analysis::AnalysisRunner,
    transaction::{
        Change, NewEdge, NewNode,
        Reference::{Added, Existing},
        RevisionIntent, prepare,
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
    Change::AddNode(NewNode {
        ty_name: name.into(),
        body: Body::default(),
        parent,
        intent: parent.map(|_| RevisionIntent::Branch),
    })
}
fn append(graph: &mut LifeCoreList, changes: &[Change]) -> Vec<EntryId> {
    let plan = prepare(graph, changes, &mut AnalysisRunner::default())
        .unwrap()
        .accepted()
        .unwrap();
    let ids = plan.added();
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
            Change::AddEdge(NewEdge {
                from: Added(0),
                ty_name: "link".into(),
                to: Added(1),
            }),
            node("A2", Some(Added(0))),
            node("A3", Some(Added(0))),
            Change::AddEdge(NewEdge {
                from: Added(3),
                ty_name: "link".into(),
                to: Added(1),
            }),
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
            Change::AddEdge(NewEdge {
                from: Added(0),
                ty_name: "link".into(),
                to: Added(1),
            }),
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
    let first = prepare(&graph, &[node("B", None)], &mut AnalysisRunner::default())
        .unwrap()
        .accepted()
        .unwrap();
    let second = prepare(&graph, &[node("B", None)], &mut AnalysisRunner::default())
        .unwrap()
        .accepted()
        .unwrap();
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
            Change::AddEdge(NewEdge {
                from: Added(0),
                ty_name: "x".into(),
                to: Added(0),
            }),
        ],
        vec![node("A", Some(Added(0)))],
        vec![
            node("A", None),
            node("B", None),
            Change::AddEdge(NewEdge {
                from: Added(0),
                ty_name: "x".into(),
                to: Added(1),
            }),
            node("Child", Some(Added(2))),
        ],
        vec![
            node("A", None),
            node("B", None),
            node("A2", Some(Added(0))),
            Change::AddEdge(NewEdge {
                from: Added(0),
                ty_name: "x".into(),
                to: Added(1),
            }),
        ],
        vec![
            node("A", None),
            node("B", None),
            Change::AddEdge(NewEdge {
                from: Added(0),
                ty_name: "x".into(),
                to: Added(1),
            }),
            Change::AddEdge(NewEdge {
                from: Added(0),
                ty_name: "x".into(),
                to: Added(1),
            }),
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
    .unwrap()
    .accepted()
    .unwrap();
    let [a, b] = candidate.added()[..] else {
        unreachable!()
    };
    assert_eq!(a.partial_cmp(&b), Some(a.cmp(&b)));
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
    assert_ne!(Payload::Integer(1), Payload::Boolean(true));
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
    wrong["entries"][1]["Node"]["id"]["position"] = "99".into();
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

#[test]
fn payloads_export_canonical_envelopes_and_accept_bare_integer_shorthand() {
    let time: crate::Timestamp = "2026-09-30T17:00:00.123456789Z".parse().unwrap();
    let value = Payload::Record(Body::from([
        ("title".into(), "Submit report".into()),
        ("priority".into(), 2.into()),
        (
            "labels".into(),
            Payload::List(vec!["work".into(), i64::MIN.into()]),
        ),
        ("deadline".into(), time.into()),
        ("completed".into(), false.into()),
        ("empty_list".into(), Payload::List(vec![])),
        ("empty_record".into(), Payload::Record(Body::new())),
    ]));
    let canonical = serde_json::json!({"kind": "record", "value": {
        "title": "Submit report",
        "priority": {"kind": "integer", "value": "2"},
        "labels": ["work", {"kind": "integer", "value": "-9223372036854775808"}],
        "deadline": {"kind": "timestamp", "value": "2026-09-30T17:00:00.123456789Z"},
        "completed": {"kind": "boolean", "value": false},
        "empty_list": [],
        "empty_record": {"kind": "record", "value": {}},
    }});
    assert_eq!(serde_json::to_value(&value).unwrap(), canonical);
    assert_eq!(serde_json::from_value::<Payload>(canonical).unwrap(), value);
    //A Shorthand works at every nesting level and at both i64 extrema, and the
    //A envelope may list `value` before `kind`.
    let shorthand = r#"{"value": {
        "title": "Submit report",
        "priority": 2,
        "labels": ["work", -9223372036854775808],
        "deadline": {"value": "2026-09-30T17:00:00.123456789Z", "kind": "timestamp"},
        "completed": {"kind": "boolean", "value": false},
        "empty_list": [],
        "empty_record": {"kind": "record", "value": {}}
    }, "kind": "record"}"#;
    assert_eq!(serde_json::from_str::<Payload>(shorthand).unwrap(), value);
    assert_eq!(
        serde_json::from_str::<Payload>("9223372036854775807").unwrap(),
        Payload::Integer(i64::MAX)
    );
    assert_eq!(
        serde_json::to_string(&Payload::Integer(i64::MAX)).unwrap(),
        r#"{"kind":"integer","value":"9223372036854775807"}"#
    );
    //A Quoted digits outside an envelope and record fields named like tags
    //A remain ordinary values.
    assert_eq!(
        serde_json::from_str::<Payload>(r#""42""#).unwrap(),
        Payload::Text("42".into())
    );
    let tag_like = Payload::Record(Body::from([
        ("kind".into(), "integer".into()),
        ("value".into(), "1".into()),
    ]));
    let encoded = serde_json::to_string(&tag_like).unwrap();
    assert_eq!(serde_json::from_str::<Payload>(&encoded).unwrap(), tag_like);
}

#[test]
fn malformed_payload_envelopes_and_integers_are_rejected_without_coercion() {
    for input in [
        "9223372036854775808",
        "-9223372036854775809",
        "18446744073709551616",
        "1.5",
        "1e3",
        "true",
        "null",
        r#"{"kind":"integer","value":"9223372036854775808"}"#,
        r#"{"kind":"integer","value":42}"#,
        r#"{"kind":"integer","value":"+1"}"#,
        r#"{"kind":"integer","value":" 1"}"#,
        r#"{"kind":"integer","value":"1.0"}"#,
        r#"{"kind":"integer","value":""}"#,
        r#"{"kind":"integer","value":"0x10"}"#,
        r#"{"kind":"integer","value":"-0"}"#,
        r#"{"kind":"integer","value":"00"}"#,
        r#"{"kind":"integer","value":"007"}"#,
        r#"{"kind":"integer","value":"-07"}"#,
        "-0",
        r#"{"kind":"number","value":"1"}"#,
        r#"{"kind":"text","value":"x"}"#,
        r#"{"kind":"list","value":[]}"#,
        r#"{"kind":"boolean","value":"true"}"#,
        r#"{"kind":"timestamp","value":"yesterday"}"#,
        r#"{"kind":"timestamp","value":1}"#,
        r#"{"kind":"record","value":[]}"#,
        r#"{"kind":"record"}"#,
        r#"{"value":"1"}"#,
        r#"{"kind":"integer","value":"1","extra":true}"#,
        r#"{"kind":"integer","kind":"integer","value":"1"}"#,
        r#"{"kind":"record","value":{"a":"x","a":"y"}}"#,
        r#"{"title":"not wrapped"}"#,
    ] {
        assert!(
            serde_json::from_str::<Payload>(input).is_err(),
            "accepted {input}"
        );
    }
    //A Zero has exactly one spelling in either input form.
    for input in [r#"{"kind":"integer","value":"0"}"#, "0"] {
        assert_eq!(
            serde_json::from_str::<Payload>(input).unwrap(),
            Payload::Integer(0)
        );
    }
}

#[test]
fn bodies_reject_duplicate_fields_wherever_they_appear() {
    let origin = uuid::Uuid::new_v4();
    let duplicated = serde_json::json!({"a": "x"})
        .to_string()
        .replace('}', r#","a":"y"}"#);
    let node = format!(
        r#"{{"id":{{"origin":"{origin}","position":"0"}},"ty_name":"T","body":{duplicated},"parent":null}}"#
    );
    let new_node = format!(r#"{{"ty_name":"T","body":{duplicated},"parent":null,"intent":null}}"#);
    let change = format!(r#"{{"AddNode":{new_node}}}"#);
    let graph = format!(r#"{{"id":"{origin}","entries":[{{"Node":{node}}}],"suite":[]}}"#);
    assert!(serde_json::from_str::<Body>(&duplicated).is_err());
    assert!(serde_json::from_str::<crate::Node>(&node).is_err());
    assert!(serde_json::from_str::<NewNode>(&new_node).is_err());
    assert!(serde_json::from_str::<Change>(&change).is_err());
    assert!(serde_json::from_str::<LifeCoreList>(&graph).is_err());
    //A The same documents without the repeated field decode.
    let single = |text: &str| text.replace(r#","a":"y""#, "");
    assert_eq!(
        serde_json::from_str::<Body>(&single(&duplicated)).unwrap(),
        Body::from([("a".into(), "x".into())])
    );
    serde_json::from_str::<crate::Node>(&single(&node)).unwrap();
    serde_json::from_str::<Change>(&single(&change)).unwrap();
    serde_json::from_str::<LifeCoreList>(&single(&graph)).unwrap();
}

#[test]
fn timestamp_payloads_follow_jiff_serde_and_keep_nanoseconds() {
    for time in [
        crate::Timestamp::MIN,
        crate::Timestamp::MAX,
        crate::Timestamp::UNIX_EPOCH,
        "1969-12-31T23:59:59.999999999Z".parse().unwrap(),
    ] {
        let encoded = serde_json::to_value(Payload::Timestamp(time)).unwrap();
        assert_eq!(encoded["value"], serde_json::to_value(time).unwrap());
        assert_eq!(
            serde_json::from_value::<Payload>(encoded).unwrap(),
            Payload::Timestamp(time)
        );
    }
    //A Jiff accepts an offset and represents the same instant; LifeCore adds no
    //A normalization profile of its own.
    let offset: Payload =
        serde_json::from_str(r#"{"kind":"timestamp","value":"2026-09-30T20:00:00+03:00"}"#)
            .unwrap();
    assert_eq!(
        offset,
        Payload::Timestamp("2026-09-30T17:00:00Z".parse().unwrap())
    );
}

#[test]
fn large_control_integers_are_decimal_strings_in_identities_and_views() {
    let mut graph = LifeCoreList::default();
    let ids = append(&mut graph, &[node("A", None), node("B", Some(Added(0)))]);
    let encoded = serde_json::to_value(ids[1]).unwrap();
    assert_eq!(encoded["position"], "1");
    assert_eq!(
        serde_json::from_value::<EntryId>(encoded.clone()).unwrap(),
        ids[1]
    );
    let view = graph.view().prefix(1).unwrap().id();
    let encoded_view = serde_json::to_value(view).unwrap();
    assert_eq!(encoded_view["end_exclusive"], "1");
    assert_eq!(encoded_view["head"]["position"], "0");
    assert_eq!(
        serde_json::from_value::<crate::ViewId>(encoded_view).unwrap(),
        view
    );
    let stored = serde_json::to_value(&graph).unwrap();
    assert_eq!(stored["entries"][1]["Node"]["parent"]["position"], "0");
    let mut zero = encoded.clone();
    zero["position"] = "0".into();
    assert_eq!(
        serde_json::from_value::<EntryId>(zero).unwrap().position(),
        0
    );
    //A The static type bounds the range; bare numbers, signs, and non-canonical
    //A spellings are not accepted, since a position is part of an identity.
    let mut far = encoded.clone();
    far["position"] = usize::MAX.to_string().into();
    assert_eq!(
        serde_json::from_value::<EntryId>(far).unwrap().position(),
        usize::MAX
    );
    for position in [
        serde_json::json!(1),
        serde_json::json!("-1"),
        serde_json::json!("+1"),
        serde_json::json!("18446744073709551616"),
        serde_json::json!(""),
        serde_json::json!("01"),
        serde_json::json!("-0"),
        serde_json::json!("00"),
    ] {
        let mut changed = encoded.clone();
        changed["position"] = position.clone();
        assert!(
            serde_json::from_value::<EntryId>(changed).is_err(),
            "accepted {position}"
        );
    }
}
