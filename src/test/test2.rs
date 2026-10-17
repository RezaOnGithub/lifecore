use crate::{
    Body, Edge, Entry, EntryId, LifeCoreList, Node, Payload, PayloadKind, View, ViewError,
    analysis::AnalysisRunner,
    transaction::{
        Change, EntryChange, NewEdge, NewNode,
        Reference::{Added, Existing},
        RevisionIntent, prepare,
    },
    typing::{
        FieldDecl, FieldIssueKind, FieldStatus, PathPart, SchemaError, SchemaErrorKind, Type,
        TypeResolution, ValueType, kind_word, path_text,
    },
};
use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

fn node(name: &str, predecessor: Option<crate::transaction::Reference>) -> Change {
    Change::Entry(EntryChange::Node(NewNode {
        ty_name: name.into(),
        payload: Body::default(),
        predecessor,
        intent: predecessor.map(|_| RevisionIntent::Branch),
    }))
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
    assert_eq!(view.resolve_type("Type"), TypeResolution::Builtin);
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
            Change::Entry(EntryChange::Edge(NewEdge {
                from: Added(0),
                ty_name: "link".into(),
                to: Added(1),
            })),
            node("A2", Some(Added(0))),
            node("A3", Some(Added(0))),
            Change::Entry(EntryChange::Edge(NewEdge {
                from: Added(3),
                ty_name: "link".into(),
                to: Added(1),
            })),
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
    assert_eq!(middle.node(ids[3]).unwrap().predecessor, Some(ids[0]));
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
            Change::Entry(EntryChange::Edge(NewEdge {
                from: Added(0),
                ty_name: "link".into(),
                to: Added(1),
            })),
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
        // From inside the tree, only the subtree below the start.
        assert_eq!(
            view.descendants(ids[3]).map(|n| n.id).collect::<Vec<_>>(),
            if end_exclusive > 4 {
                vec![ids[4]]
            } else {
                vec![]
            }
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
            Change::Entry(EntryChange::Edge(NewEdge {
                from: Added(0),
                ty_name: "x".into(),
                to: Added(0),
            })),
        ],
        vec![node("A", Some(Added(0)))],
        vec![
            node("A", None),
            node("B", None),
            Change::Entry(EntryChange::Edge(NewEdge {
                from: Added(0),
                ty_name: "x".into(),
                to: Added(1),
            })),
            node("Child", Some(Added(2))),
        ],
        vec![
            node("A", None),
            node("B", None),
            node("A2", Some(Added(0))),
            Change::Entry(EntryChange::Edge(NewEdge {
                from: Added(0),
                ty_name: "x".into(),
                to: Added(1),
            })),
        ],
        vec![
            node("A", None),
            node("B", None),
            Change::Entry(EntryChange::Edge(NewEdge {
                from: Added(0),
                ty_name: "x".into(),
                to: Added(1),
            })),
            Change::Entry(EntryChange::Edge(NewEdge {
                from: Added(0),
                ty_name: "x".into(),
                to: Added(1),
            })),
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
    let record = ValueType::Record {
        fields: [(
            "label".into(),
            FieldDecl {
                status: FieldStatus::Mandatory,
                shape: ValueType::Text,
            },
        )]
        .into(),
    };
    let ty = Type::new(
        "Thing",
        &[(
            "rows",
            (
                ValueType::List {
                    element: Box::new(record),
                },
                FieldStatus::Mandatory,
            ),
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

// A declaration list of words, optionally ending in a sub-field record.
fn words(words: &[&str], fields: Option<Body>) -> Payload {
    let mut list = words
        .iter()
        .map(|&word| word.into())
        .collect::<Vec<Payload>>();
    list.extend(fields.map(Payload::Record));
    Payload::List(list)
}

// `levels` record-shaped fields nested through a sub-field `r`, around a text.
fn nested_records(levels: usize) -> Payload {
    (0..levels).fold(words(&["optional", "text"], None), |inner, _| {
        words(
            &["optional", "record"],
            Some(Body::from([("r".into(), inner)])),
        )
    })
}

#[test]
fn declarations_encode_fields_as_word_lists_and_round_trip_every_shape() {
    use FieldStatus::*;
    use ValueType::*;
    let list = |element| List {
        element: Box::new(element),
    };
    let owner = Record {
        fields: [(
            "owner".into(),
            FieldDecl {
                status: Mandatory,
                shape: Text,
            },
        )]
        .into(),
    };
    let row = Record {
        fields: [(
            "inner".into(),
            FieldDecl {
                status: Encouraged,
                shape: owner.clone(),
            },
        )]
        .into(),
    };
    let ty = Type::new(
        "Task",
        &[
            ("title", (Text, Mandatory)),
            ("count", (Integer, Encouraged)),
            ("done", (Boolean, Deprecated)),
            ("due", (Timestamp, Optional)),
            ("tags", (list(Text), Optional)),
            ("grid", (list(list(Integer)), Optional)),
            ("details", (owner, Optional)),
            ("rows", (list(row), Mandatory)),
        ],
        &["DependsOn"],
        &["Explains"],
        &["Note", "Task2"],
    );
    let owner_fields = || Body::from([("owner".into(), words(&["mandatory", "text"], None))]);
    let expected = Body::from([
        ("name".into(), "Task".into()),
        (
            "fields".into(),
            Payload::Record(Body::from([
                ("title".into(), words(&["mandatory", "text"], None)),
                ("count".into(), words(&["encouraged", "integer"], None)),
                ("done".into(), words(&["deprecated", "boolean"], None)),
                ("due".into(), words(&["optional", "timestamp"], None)),
                ("tags".into(), words(&["optional", "list", "text"], None)),
                (
                    "grid".into(),
                    words(&["optional", "list", "list", "integer"], None),
                ),
                (
                    "details".into(),
                    words(&["optional", "record"], Some(owner_fields())),
                ),
                (
                    "rows".into(),
                    words(
                        &["mandatory", "list", "record"],
                        Some(Body::from([(
                            "inner".into(),
                            words(&["encouraged", "record"], Some(owner_fields())),
                        )])),
                    ),
                ),
            ])),
        ),
        ("outgoing".into(), words(&["DependsOn"], None)),
        ("incoming".into(), words(&["Explains"], None)),
        ("successors".into(), words(&["Note", "Task2"], None)),
    ]);
    assert_eq!(ty.to_body(), expected);
    assert_eq!(Type::from_body(&expected).unwrap(), ty);
    // Every collection but the name may be omitted, and then is empty.
    assert_eq!(
        Type::from_body(&Body::from([("name".into(), "Bare".into())])).unwrap(),
        Type::new("Bare", &[], &[], &[], &[])
    );
    // The deepest accepted nesting, through lists and through records.
    let deep = (0..64).fold(Text, |element, _| list(element));
    let ty = Type::new("Deep", &[("d", (deep, Optional))], &[], &[], &[]);
    assert_eq!(Type::from_body(&ty.to_body()).unwrap(), ty);
    let body = Body::from([
        ("name".into(), "Deep".into()),
        (
            "fields".into(),
            Payload::Record(Body::from([("d".into(), nested_records(64))])),
        ),
    ]);
    let ty = Type::from_body(&body).unwrap();
    assert_eq!(ty.to_body()["fields"], body["fields"]);
}

#[test]
fn malformed_declarations_report_the_offending_element_and_kind() {
    use PathPart::{Field, Index};
    use PayloadKind as K;
    use SchemaErrorKind::*;
    let wrong = |expected, actual| WrongShape { expected, actual };
    let field = |name: &str| Field(name.into());
    let at = |rest: &[PathPart]| [vec![field("fields"), field("a")], rest.to_vec()].concat();
    let lists = |count: usize| {
        let mut list = vec!["optional"];
        list.extend(std::iter::repeat_n("list", count));
        list.push("text");
        words(&list, None)
    };
    let mut below_records = (0..64)
        .flat_map(|_| [Index(2), field("r")])
        .collect::<Vec<_>>();
    below_records.push(Index(1));
    let declarations = [
        ("text".into(), at(&[]), wrong(K::List, K::Text)),
        // The per-field record that preceded word lists.
        (
            Payload::Record(Body::from([
                (
                    "shape".into(),
                    Payload::Record(Body::from([("kind".into(), "text".into())])),
                ),
                ("presence".into(), "mandatory".into()),
            ])),
            at(&[]),
            wrong(K::List, K::Record),
        ),
        (words(&[], None), at(&[Index(0)]), MissingPresence),
        (
            Payload::List(vec![1.into(), "text".into()]),
            at(&[Index(0)]),
            wrong(K::Text, K::Integer),
        ),
        (
            words(&["required", "text"], None),
            at(&[Index(0)]),
            UnknownPresence {
                word: "required".into(),
            },
        ),
        // Presence comes first, so a leading shape word is an unknown presence.
        (
            words(&["text", "optional"], None),
            at(&[Index(0)]),
            UnknownPresence {
                word: "text".into(),
            },
        ),
        (words(&["optional"], None), at(&[Index(1)]), MissingShape),
        (
            words(&["optional", "list"], None),
            at(&[Index(2)]),
            MissingShape,
        ),
        (
            words(&["optional", "float"], None),
            at(&[Index(1)]),
            UnknownShape {
                word: "float".into(),
            },
        ),
        (
            words(&["optional", "list", "Text"], None),
            at(&[Index(2)]),
            UnknownShape {
                word: "Text".into(),
            },
        ),
        (
            Payload::List(vec!["optional".into(), true.into()]),
            at(&[Index(1)]),
            wrong(K::Text, K::Boolean),
        ),
        (
            words(&["optional"], Some(Body::new())),
            at(&[Index(1)]),
            wrong(K::Text, K::Record),
        ),
        (
            words(&["optional", "record"], None),
            at(&[Index(2)]),
            MissingRecordFields,
        ),
        (
            words(&["optional", "record", "text"], None),
            at(&[Index(2)]),
            wrong(K::Record, K::Text),
        ),
        (
            Payload::List(vec![
                "optional".into(),
                "record".into(),
                Payload::List(vec![]),
            ]),
            at(&[Index(2)]),
            wrong(K::Record, K::List),
        ),
        (
            words(&["optional", "text", "text"], None),
            at(&[Index(2)]),
            TrailingElements,
        ),
        (
            Payload::List(vec![
                "optional".into(),
                "record".into(),
                Payload::Record(Body::new()),
                Payload::Record(Body::new()),
            ]),
            at(&[Index(3)]),
            TrailingElements,
        ),
        (
            Payload::List(vec![
                "optional".into(),
                "record".into(),
                Payload::Record(Body::new()),
                "text".into(),
            ]),
            at(&[Index(3)]),
            TrailingElements,
        ),
        // Errors inside a sub-field record continue the path through it.
        (
            words(
                &["optional", "list", "record"],
                Some(Body::from([(
                    "b".into(),
                    words(&["sometimes", "text"], None),
                )])),
            ),
            at(&[Index(3), field("b"), Index(0)]),
            UnknownPresence {
                word: "sometimes".into(),
            },
        ),
        (lists(65), at(&[Index(65)]), TooDeep),
        // A flat list far deeper than the stack could recurse is still reported.
        (lists(100_000), at(&[Index(65)]), TooDeep),
        (nested_records(65), at(&below_records), TooDeep),
    ];
    for (declaration, path, kind) in declarations {
        let body = Body::from([
            ("name".into(), "T".into()),
            (
                "fields".into(),
                Payload::Record(Body::from([("a".into(), declaration)])),
            ),
        ]);
        assert_eq!(
            Type::from_body(&body).unwrap_err(),
            SchemaError { path, kind }
        );
    }
    let named =
        |key: &str, value: Payload| Body::from([("name".into(), "T".into()), (key.into(), value)]);
    let bodies = [
        (Body::new(), vec![field("name")], MissingField),
        (
            Body::from([("name".into(), 1.into())]),
            vec![field("name")],
            wrong(K::Text, K::Integer),
        ),
        (
            named("fields", Payload::List(vec![])),
            vec![field("fields")],
            wrong(K::Record, K::List),
        ),
        (
            named("outgoing", "A".into()),
            vec![field("outgoing")],
            wrong(K::List, K::Text),
        ),
        (
            named("incoming", Payload::List(vec![1.into()])),
            vec![field("incoming"), Index(0)],
            wrong(K::Text, K::Integer),
        ),
        (
            named("successors", words(&["A", "B", "A"], None)),
            vec![field("successors"), Index(2)],
            DuplicatePermission { name: "A".into() },
        ),
        // The former names of `fields` and `successors` are unknown keys.
        (
            named("body", Payload::Record(Body::new())),
            vec![field("body")],
            UnknownField,
        ),
        (
            named("additional_override", Payload::List(vec![])),
            vec![field("additional_override")],
            UnknownField,
        ),
    ];
    for (body, path, kind) in bodies {
        assert_eq!(
            Type::from_body(&body).unwrap_err(),
            SchemaError { path, kind }
        );
    }
}

#[test]
fn schema_errors_name_the_path_and_the_problem_in_words() {
    use PathPart::{Field, Index};
    let field = |name: &str| Field(name.into());
    assert_eq!(path_text(&[]), "");
    assert_eq!(
        path_text(&[field("details"), field("owner")]),
        "details.owner"
    );
    assert_eq!(path_text(&[field("tags"), Index(1)]), "tags[1]");
    assert_eq!(path_text(&[Index(0), field("_x-1")]), "[0]._x-1");
    // Names outside the identifier grammar are quoted.
    assert_eq!(
        path_text(&[field("due date"), field("a.b"), field("1a"), field("")]),
        r#""due date"."a.b"."1a"."""#
    );
    let declared = |field: &str, declaration: Payload| {
        let body = Body::from([
            ("name".into(), "T".into()),
            (
                "fields".into(),
                Payload::Record(Body::from([(field.into(), declaration)])),
            ),
        ]);
        Type::from_body(&body).unwrap_err().to_string()
    };
    assert_eq!(
        declared("title", words(&["optional", "txt"], None)),
        r#"invalid schema at fields.title[1]: unknown shape word "txt""#
    );
    assert_eq!(
        declared("due date", words(&["sometimes", "text"], None)),
        r#"invalid schema at fields."due date"[0]: unknown presence word "sometimes""#
    );
    assert_eq!(
        declared("a", "text".into()),
        "invalid schema at fields.a: should be list, found text"
    );
    assert_eq!(
        declared("a", words(&[], None)),
        "invalid schema at fields.a[0]: missing presence word"
    );
    assert_eq!(
        declared("a", words(&["optional", "list"], None)),
        "invalid schema at fields.a[2]: missing shape word"
    );
    assert_eq!(
        declared("a", words(&["optional", "record"], None)),
        r#"invalid schema at fields.a[2]: missing field declarations after "record""#
    );
    assert_eq!(
        declared("a", words(&["optional", "text", "text"], None)),
        "invalid schema at fields.a[2]: unexpected words after the shape"
    );
    assert_eq!(
        declared(
            "a",
            words(
                &["optional", "list", "record"],
                Some(Body::from([(
                    "b".into(),
                    words(&["optional", "txt"], None)
                )])),
            ),
        ),
        r#"invalid schema at fields.a[3].b[1]: unknown shape word "txt""#
    );
    let mut deep = vec!["optional"];
    deep.extend(std::iter::repeat_n("list", 65));
    deep.push("text");
    assert_eq!(
        declared("a", words(&deep, None)),
        "invalid schema at fields.a[65]: shape nested more than 64 levels deep"
    );
    let error = |body: Body| Type::from_body(&body).unwrap_err().to_string();
    assert_eq!(
        error(Body::new()),
        "invalid schema at name: field is missing"
    );
    assert_eq!(
        error(Body::from([("name".into(), 1.into())])),
        "invalid schema at name: should be text, found integer"
    );
    assert_eq!(
        error(Body::from([
            ("name".into(), "T".into()),
            ("colour".into(), "red".into()),
        ])),
        "invalid schema at colour: unknown field"
    );
    assert_eq!(
        error(Body::from([
            ("name".into(), "T".into()),
            ("successors".into(), words(&["A", "B", "A"], None)),
        ])),
        r#"invalid schema at successors[2]: duplicate permission "A""#
    );
    let rootless = SchemaError {
        path: vec![],
        kind: SchemaErrorKind::UnknownField,
    };
    assert_eq!(rootless.to_string(), "invalid schema: unknown field");
    assert_eq!(kind_word(PayloadKind::Timestamp), "timestamp");
}

#[test]
fn deserialization_rejects_cyclic_forward_and_mismatched_identities_before_inspection() {
    let mut graph = LifeCoreList::default();
    append(&mut graph, &[node("A", None), node("B", Some(Added(0)))]);
    let original = serde_json::to_value(&graph).unwrap();
    let mut cyclic = original.clone();
    cyclic["entries"][0]["predecessor"] = cyclic["entries"][0]["id"].clone();
    assert!(serde_json::from_value::<LifeCoreList>(cyclic).is_err());
    let mut forward = original.clone();
    forward["entries"][0]["predecessor"] = forward["entries"][1]["id"].clone();
    assert!(serde_json::from_value::<LifeCoreList>(forward).is_err());
    let mut wrong = original;
    wrong["entries"][1]["id"]["position"] = "99".into();
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
        // Rebuild the truncated history through deserialization, so its derived
        // staleness index is built from the retained entries alone.
        let mut stored = serde_json::to_value(&full).unwrap();
        stored["entries"]
            .as_array_mut()
            .unwrap()
            .truncate(end_exclusive);
        let truncated: LifeCoreList = serde_json::from_value(stored).unwrap();
        let bounded = full.view().prefix(end_exclusive).unwrap();
        assert_eq!(bounded.id(), truncated.view().id());
        // Debug output must not reveal first-child positions past the boundary.
        assert_eq!(format!("{bounded:?}"), format!("{:?}", truncated.view()));
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
    // Shorthand works at every nesting level and at both i64 extrema, and the
    // envelope may list `value` before `kind`.
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
    // Quoted digits outside an envelope and record fields named like tags
    // remain ordinary values.
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
    // Zero has exactly one spelling in either input form.
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
        r#"{{"id":{{"origin":"{origin}","position":"0"}},"ty_name":"T","payload":{duplicated}}}"#
    );
    let new_node = format!(r#"{{"ty_name":"T","payload":{duplicated}}}"#);
    let entry_change = format!(r#"{{"kind":"node","ty_name":"T","payload":{duplicated}}}"#);
    let entry = node.replacen('{', r#"{"kind":"node","#, 1);
    let graph = format!(r#"{{"id":"{origin}","entries":[{entry}],"suite":[]}}"#);
    assert!(serde_json::from_str::<Body>(&duplicated).is_err());
    assert!(serde_json::from_str::<crate::Node>(&node).is_err());
    assert!(serde_json::from_str::<NewNode>(&new_node).is_err());
    assert!(serde_json::from_str::<EntryChange>(&entry_change).is_err());
    assert!(serde_json::from_str::<LifeCoreList>(&graph).is_err());
    // The same documents without the repeated field decode.
    let single = |text: &str| text.replace(r#","a":"y""#, "");
    assert_eq!(
        serde_json::from_str::<Body>(&single(&duplicated)).unwrap(),
        Body::from([("a".into(), "x".into())])
    );
    serde_json::from_str::<crate::Node>(&single(&node)).unwrap();
    serde_json::from_str::<EntryChange>(&single(&entry_change)).unwrap();
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
    // Jiff accepts an offset and represents the same instant; LifeCore adds no
    // normalization profile of its own.
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
    assert_eq!(stored["entries"][1]["predecessor"]["position"], "0");
    let mut zero = encoded.clone();
    zero["position"] = "0".into();
    assert_eq!(
        serde_json::from_value::<EntryId>(zero).unwrap().position(),
        0
    );
    // The static type bounds the range; bare numbers, signs, and non-canonical
    // spellings are not accepted, since a position is part of an identity.
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

// The scan `View::is_stale` used before the staleness index, kept as its reference.
fn scanned_is_stale(view: View<'_>, id: EntryId) -> Option<bool> {
    match view.get(id)? {
    Entry::Node(_) => Some(view.nodes().any(|n| n.predecessor == Some(id))),
    Entry::Edge(e) => Some(scanned_is_stale(view, e.from)? || scanned_is_stale(view, e.to)?),
    }
}

// Probes every identity known to the test, visible or not, at every prefix.
fn assert_staleness_matches_scan(view: View<'_>, probes: &[EntryId]) {
    for end_exclusive in 0..=view.len() {
        let prefix = view.prefix(end_exclusive).unwrap();
        for id in view.entries().map(Entry::id).chain(probes.iter().copied()) {
            assert_eq!(
                prefix.is_stale(id),
                scanned_is_stale(prefix, id),
                "{id:?} at boundary {end_exclusive}"
            );
        }
        assert_eq!(
            prefix.fresh_nodes().collect::<Vec<_>>(),
            prefix
                .nodes()
                .filter(|n| scanned_is_stale(prefix, n.id) == Some(false))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            prefix.fresh_edges().collect::<Vec<_>>(),
            prefix
                .edges()
                .filter(|e| scanned_is_stale(prefix, e.id) == Some(false))
                .collect::<Vec<_>>()
        );
    }
}

fn edge(from: crate::transaction::Reference, to: crate::transaction::Reference) -> Change {
    Change::Entry(EntryChange::Edge(NewEdge {
        from,
        ty_name: "link".into(),
        to,
    }))
}

// Revisions, a split, branching from a stale node, and edges whose endpoints
// are superseded later, committed across two plans.
fn staleness_fixture() -> (LifeCoreList, Vec<EntryId>) {
    let mut graph = LifeCoreList::default();
    let mut ids = append(
        &mut graph,
        &[
            node("A", None),
            node("B", None),
            node("C", None),
            edge(Added(0), Added(1)),
            edge(Added(1), Added(2)),
            node("A2", Some(Added(0))),
            node("A3", Some(Added(0))),
            node("A4", Some(Added(5))),
            edge(Added(7), Added(1)),
        ],
    );
    ids.extend(append(
        &mut graph,
        &[
            node("B2", Some(Existing(ids[1]))),
            node("B3", Some(Existing(ids[1]))),
            edge(Existing(ids[2]), Added(0)),
            node("C2", Some(Existing(ids[2]))),
            edge(Existing(ids[6]), Added(3)),
        ],
    ));
    (graph, ids)
}

#[test]
fn indexed_staleness_matches_the_scan_for_every_entry_at_every_prefix() {
    let (graph, ids) = staleness_fixture();
    let mut other = LifeCoreList::default();
    let foreign = append(&mut other, &[node("A", None), node("A2", Some(Added(0)))]);
    assert_staleness_matches_scan(graph.view(), &foreign);
    // Spot checks around the relevant appends: A before and after its first
    // child at 5, a later sibling leaving that boundary unchanged, and edges 3
    // and 8 becoming stale through their source and target respectively.
    let view = graph.view();
    for (end_exclusive, a, b, edge_ab, edge_a4b) in [
        (5, false, false, Some(false), None),
        (6, true, false, Some(true), None),
        (7, true, false, Some(true), None),
        (9, true, false, Some(true), Some(false)),
        (10, true, true, Some(true), Some(true)),
        (14, true, true, Some(true), Some(true)),
    ] {
        let prefix = view.prefix(end_exclusive).unwrap();
        assert_eq!(prefix.is_stale(ids[0]), Some(a));
        assert_eq!(prefix.is_stale(ids[1]), Some(b));
        assert_eq!(prefix.is_stale(ids[3]), edge_ab);
        assert_eq!(prefix.is_stale(ids[8]), edge_a4b);
    }
    assert_eq!(view.prefix(9).unwrap().is_stale(ids[9]), None);
    assert_eq!(
        graph.staleness(),
        [
            Some(5),
            Some(9),
            Some(12),
            None,
            None,
            Some(7),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ]
    );
}

#[test]
fn staged_and_independent_candidates_extend_their_own_index() {
    let (graph, ids) = staleness_fixture();
    let before = graph.staleness().to_vec();
    let staged = prepare(
        &graph,
        &[
            node("A5", Some(Existing(ids[7]))),
            edge(Added(0), Existing(ids[10])),
            node("D", None),
            node("D2", Some(Added(2))),
            edge(Added(0), Added(3)),
            node("A6", Some(Added(0))),
        ],
        &mut AnalysisRunner::default(),
    )
    .unwrap()
    .accepted()
    .unwrap();
    let independent = prepare(
        &graph,
        &[node("C3", Some(Existing(ids[12])))],
        &mut AnalysisRunner::default(),
    )
    .unwrap()
    .accepted()
    .unwrap();
    assert_staleness_matches_scan(staged.view(), &independent.added());
    assert_staleness_matches_scan(independent.view(), &staged.added());
    // Staging clones the base; neither candidate changes it or the other.
    assert_eq!(graph.staleness(), before);
    assert_eq!(graph.view().is_stale(ids[7]), Some(false));
    assert_eq!(graph.view().is_stale(ids[12]), Some(false));
    assert_eq!(staged.view().is_stale(ids[7]), Some(true));
    assert_eq!(staged.view().is_stale(ids[12]), Some(false));
    assert_eq!(independent.view().is_stale(ids[7]), Some(false));
    assert_eq!(independent.view().is_stale(ids[12]), Some(true));
    let added = staged.added();
    assert_eq!(
        staged.view().prefix(19).unwrap().is_stale(added[0]),
        Some(false)
    );
    assert_eq!(staged.view().is_stale(added[0]), Some(true));
    assert_eq!(staged.view().is_stale(added[4]), Some(true));
    let mut committed = graph.clone();
    staged.commit(&mut committed).unwrap();
    assert_eq!(
        committed.staleness()[..14],
        {
            let mut extended = before.clone();
            extended[7] = Some(14);
            extended
        }[..]
    );
    assert_eq!(
        committed.staleness()[14..],
        [Some(19), None, Some(17), None, None, None]
    );
    assert_staleness_matches_scan(committed.view(), &[]);
}

#[test]
fn a_rejected_entry_is_never_indexed() {
    let (graph, ids) = staleness_fixture();
    let mut other = LifeCoreList::default();
    let foreign = append(&mut other, &[node("A", None), node("B", None)])[1];
    assert_eq!(foreign.position(), ids[1].position());
    // None of these parents is an exact earlier node: a foreign identity at B's
    // position, an edge, and a position past the history.
    for parent in [
        foreign,
        ids[3],
        EntryId {
            origin: ids[0].origin,
            position: 20,
        },
    ] {
        let mut staged = graph.clone();
        let entry = Entry::Node(Node {
            id: EntryId {
                origin: uuid::Uuid::new_v4(),
                position: 14,
            },
            ty_name: "Invalid".into(),
            payload: Body::new(),
            predecessor: Some(parent),
        });
        assert!(matches!(
            staged.stage(entry),
            Err(crate::BasicValidationFailure::InvalidReference { .. })
        ));
        assert_eq!(staged, graph);
        assert_eq!(staged.staleness(), graph.staleness());
    }
    // The same shape with an exact parent is indexed once validated, and the
    // next staged entry already sees that parent as stale.
    let mut staged = graph.clone();
    staged
        .stage(Entry::Node(Node {
            id: EntryId {
                origin: uuid::Uuid::new_v4(),
                position: 14,
            },
            ty_name: "Valid".into(),
            payload: Body::new(),
            predecessor: Some(ids[10]),
        }))
        .unwrap();
    assert_eq!(staged.staleness()[10], Some(14));
    assert_eq!(staged.staleness().len(), 15);
    let stale_edge = Entry::Edge(Edge {
        id: EntryId {
            origin: uuid::Uuid::new_v4(),
            position: 15,
        },
        ty_name: "link".into(),
        from: ids[10],
        to: ids[12],
    });
    assert!(matches!(
        staged.clone().stage(stale_edge),
        Err(crate::BasicValidationFailure::StaleEndpoint { .. })
    ));
}

#[test]
fn the_staleness_index_is_rebuilt_on_load_and_never_serialized() {
    let (graph, _) = staleness_fixture();
    let stored = serde_json::to_value(&graph).unwrap();
    assert_eq!(
        stored.as_object().unwrap().keys().collect::<Vec<_>>(),
        ["entries", "id", "suite"]
    );
    let loaded: LifeCoreList = serde_json::from_value(stored.clone()).unwrap();
    assert_eq!(loaded.staleness(), graph.staleness());
    assert_eq!(serde_json::to_value(&loaded).unwrap(), stored);
    assert_staleness_matches_scan(loaded.view(), &[]);
    // Debug output shows the stored history as before the index existed.
    let view = graph.view();
    assert_eq!(
        format!("{graph:?}"),
        format!(
            "LifeCoreList {{ id: {:?}, entries: {:?}, suite: {:?} }}",
            view.id().database,
            view.entries().collect::<Vec<_>>(),
            graph.analysis_suite()
        )
    );
    // A stored index cannot be supplied; a new graph starts with an empty one.
    let mut injected = stored.clone();
    injected["staleness"] = serde_json::json!([null]);
    assert!(serde_json::from_value::<LifeCoreList>(injected).is_err());
    assert!(LifeCoreList::default().staleness().is_empty());
}

// Stored entries and core operations use the proposal vocabulary: `kind`,
// `ty_name`, `predecessor`, `payload`, and lowercase `existing`/`added`
// references. A root has no `predecessor` member, and null is not accepted
// as its absence anywhere.
#[test]
fn entries_and_changes_serialize_in_the_proposal_vocabulary() {
    let mut graph = LifeCoreList::default();
    let ids = append(&mut graph, &[node("A", None), node("B", None)]);
    let child = append(&mut graph, &[node("A", Some(Existing(ids[0])))])[0];
    let edge = append(
        &mut graph,
        &[Change::Entry(EntryChange::Edge(NewEdge {
            ty_name: "Rel".into(),
            from: Existing(child),
            to: Existing(ids[1]),
        }))],
    )[0];
    let id = |id: EntryId| serde_json::to_value(id).unwrap();
    let stored = serde_json::to_value(&graph).unwrap();
    assert_eq!(
        stored["entries"],
        serde_json::json!([
            {"kind": "node", "id": id(ids[0]), "ty_name": "A", "payload": {}},
            {"kind": "node", "id": id(ids[1]), "ty_name": "B", "payload": {}},
            {"kind": "node", "id": id(child), "ty_name": "A", "predecessor": id(ids[0]), "payload": {}},
            {"kind": "edge", "id": id(edge), "ty_name": "Rel", "from": id(child), "to": id(ids[1])},
        ])
    );
    let changes = [
        EntryChange::Node(NewNode {
            ty_name: "A".into(),
            payload: Body::from([("n".into(), Payload::Integer(i64::MAX))]),
            predecessor: Some(Existing(child)),
            intent: Some(RevisionIntent::Revise),
        }),
        EntryChange::Node(NewNode {
            ty_name: "C".into(),
            payload: Body::new(),
            predecessor: None,
            intent: None,
        }),
        EntryChange::Edge(NewEdge {
            ty_name: "Rel".into(),
            from: Added(0),
            to: Added(1),
        }),
    ];
    let encoded = serde_json::to_value(&changes).unwrap();
    assert_eq!(
        encoded,
        serde_json::json!([
            {"kind": "node", "ty_name": "A", "predecessor": {"existing": id(child)}, "intent": "revise",
                "payload": {"n": {"kind": "integer", "value": "9223372036854775807"}}},
            {"kind": "node", "ty_name": "C", "payload": {}},
            {"kind": "edge", "ty_name": "Rel", "from": {"added": 0}, "to": {"added": 1}},
        ])
    );
    assert_eq!(
        serde_json::from_value::<Vec<EntryChange>>(encoded.clone()).unwrap(),
        changes
    );
    let suite = crate::analysis::SuiteConfig(vec![crate::analysis::Config {
        name: "Type_Check".into(),
        configuration: "{}".into(),
    }]);
    assert_eq!(
        serde_json::to_value(&suite).unwrap(),
        serde_json::json!([{"name": "Type_Check", "configuration": "{}"}])
    );
    // Null absences, the former core spellings, and members of the other kind
    // are all rejected.
    let mut nulls = vec![];
    let mut root = stored.clone();
    root["entries"][0]["predecessor"] = serde_json::Value::Null;
    nulls.push(serde_json::from_value::<LifeCoreList>(root).is_err());
    let mut change = encoded[1].clone();
    change["intent"] = serde_json::Value::Null;
    nulls.push(serde_json::from_value::<EntryChange>(change).is_err());
    let mut change = encoded[1].clone();
    change["predecessor"] = serde_json::Value::Null;
    nulls.push(serde_json::from_value::<EntryChange>(change).is_err());
    assert_eq!(nulls, [true, true, true]);
    for rejected in [
        serde_json::json!({"Node": {"id": id(ids[0]), "ty_name": "A", "body": {}, "parent": null}}),
        serde_json::json!({"kind": "node", "id": id(ids[0]), "ty_name": "A", "payload": {}, "from": id(ids[1])}),
        serde_json::json!({"kind": "edge", "id": id(edge), "ty_name": "Rel", "from": id(child), "to": id(ids[1]), "payload": {}}),
        serde_json::json!({"kind": "Node", "id": id(ids[0]), "ty_name": "A", "payload": {}}),
        serde_json::json!({"kind": "node", "id": id(ids[0]), "type": "A", "payload": {}}),
    ] {
        assert!(
            serde_json::from_value::<Entry>(rejected.clone()).is_err(),
            "accepted {rejected}"
        );
    }
    for rejected in [
        serde_json::json!({"AddNode": {"ty_name": "A", "body": {}, "parent": null, "intent": null}}),
        serde_json::json!({"kind": "edge", "ty_name": "Rel", "from": {"Added": 0}, "to": {"added": 1}}),
        serde_json::json!({"kind": "edge", "ty_name": "Rel", "from": {"added": "0"}, "to": {"added": 1}}),
        serde_json::json!({"kind": "edge", "type": "Rel", "from": {"added": 0}, "to": {"added": 1}}),
    ] {
        assert!(
            serde_json::from_value::<EntryChange>(rejected.clone()).is_err(),
            "accepted {rejected}"
        );
    }
    assert!(
        serde_json::from_value::<crate::analysis::SuiteConfig>(
            serde_json::json!([{"name": "Type_Check", "config": "{}"}])
        )
        .is_err()
    );
}

#[test]
fn typing_values_serialize_in_tagged_snake_case() {
    use crate::typing::Declaration;
    use serde_json::json;
    let owner = FieldDecl {
        status: FieldStatus::Mandatory,
        shape: ValueType::Text,
    };
    let ty = Type::new(
        "Task",
        &[
            (
                "tags",
                (
                    ValueType::List {
                        element: Box::new(ValueType::Timestamp),
                    },
                    FieldStatus::Optional,
                ),
            ),
            (
                "details",
                (
                    ValueType::Record {
                        fields: [("owner".into(), owner)].into(),
                    },
                    FieldStatus::Encouraged,
                ),
            ),
        ],
        &["DependsOn"],
        &[],
        &["Note"],
    );
    let encoded = json!({
        "name": "Task",
        "fields": {
            "details": {
                "status": "encouraged",
                "shape": {"kind": "record", "fields": {
                    "owner": {"status": "mandatory", "shape": {"kind": "text"}},
                }},
            },
            "tags": {
                "status": "optional",
                "shape": {"kind": "list", "element": {"kind": "timestamp"}},
            },
        },
        "outgoing": ["DependsOn"],
        "incoming": [],
        "successors": ["Note"],
    });
    assert_eq!(serde_json::to_value(&ty).unwrap(), encoded);
    assert_eq!(serde_json::from_value::<Type>(encoded).unwrap(), ty);
    let errors = [
        (
            SchemaError {
                path: vec![
                    PathPart::Field("fields".into()),
                    PathPart::Field("a".into()),
                    PathPart::Index(1),
                ],
                kind: SchemaErrorKind::UnknownShape { word: "txt".into() },
            },
            json!({"path": ["fields", "a", 1], "kind": "unknown_shape", "word": "txt"}),
        ),
        (
            SchemaError {
                path: vec![PathPart::Field("outgoing".into())],
                kind: SchemaErrorKind::WrongShape {
                    expected: PayloadKind::List,
                    actual: PayloadKind::Text,
                },
            },
            json!({"path": ["outgoing"], "kind": "wrong_shape", "expected": "list", "actual": "text"}),
        ),
        (
            SchemaError {
                path: vec![],
                kind: SchemaErrorKind::TooDeep,
            },
            json!({"path": [], "kind": "too_deep"}),
        ),
    ];
    for (error, encoded) in errors {
        assert_eq!(serde_json::to_value(&error).unwrap(), encoded);
        assert_eq!(
            serde_json::from_value::<SchemaError>(encoded).unwrap(),
            error
        );
    }
    let mut graph = LifeCoreList::default();
    let id = append(
        &mut graph,
        &[Change::Entry(EntryChange::Node(NewNode {
            ty_name: "Type".into(),
            payload: Body::new(),
            predecessor: None,
            intent: None,
        }))],
    )[0];
    let error = SchemaError {
        path: vec![PathPart::Field("name".into())],
        kind: SchemaErrorKind::MissingField,
    };
    let id_json = serde_json::to_value(id).unwrap();
    let resolutions = [
        (TypeResolution::Builtin, json!({"kind": "builtin"})),
        (TypeResolution::Missing, json!({"kind": "missing"})),
        (
            TypeResolution::Declared {
                declaration: id,
                schema: Type::new("A", &[], &[], &[], &[]),
            },
            json!({"kind": "declared", "declaration": id_json, "schema": {
                "name": "A", "fields": {}, "outgoing": [], "incoming": [], "successors": [],
            }}),
        ),
        (
            TypeResolution::Malformed {
                declaration: id,
                error: error.clone(),
            },
            json!({"kind": "malformed", "declaration": id_json,
                "error": {"path": ["name"], "kind": "missing_field"}}),
        ),
        (
            TypeResolution::Ambiguous {
                claimants: vec![
                    Declaration::Malformed {
                        declaration: id,
                        error,
                    },
                    Declaration::Parsed {
                        declaration: id,
                        schema: Type::new("A", &[], &[], &[], &[]),
                    },
                ],
            },
            json!({"kind": "ambiguous", "claimants": [
                {"kind": "malformed", "declaration": id_json,
                    "error": {"path": ["name"], "kind": "missing_field"}},
                {"kind": "parsed", "declaration": id_json, "schema": {
                    "name": "A", "fields": {}, "outgoing": [], "incoming": [], "successors": [],
                }},
            ]}),
        ),
    ];
    for (resolution, encoded) in resolutions {
        assert_eq!(serde_json::to_value(&resolution).unwrap(), encoded);
        assert_eq!(
            serde_json::from_value::<TypeResolution>(encoded).unwrap(),
            resolution
        );
    }
}

// The index files each current declaration under the `name` text it claims,
// in append order, whether or not the rest parses; a declaration without one
// claims nothing. It reads only its own view's current declarations.
#[test]
fn the_declaration_index_files_claims_by_name_text_at_each_boundary() {
    use crate::typing::Declaration;
    let declare = |payload: Body, predecessor| {
        Change::Entry(EntryChange::Node(NewNode {
            ty_name: "Type".into(),
            payload,
            predecessor,
            intent: predecessor.map(|_| RevisionIntent::Branch),
        }))
    };
    let named = |name: &str| Body::from([("name".into(), name.into())]);
    let mut colourful = named("A");
    colourful.insert("colour".into(), "red".into());
    let mut graph = LifeCoreList::default();
    let ids = append(
        &mut graph,
        &[
            declare(named("A"), None),
            declare(Body::from([("name".into(), 1.into())]), None),
            declare(named(""), None),
            declare(colourful, None),
            declare(named("B"), Some(Added(0))),
        ],
    );
    let parsed = |at: usize, name: &str| Declaration::Parsed {
        declaration: ids[at],
        schema: Type::new(name, &[], &[], &[], &[]),
    };
    let nameless = Declaration::Malformed {
        declaration: ids[1],
        error: SchemaError {
            path: vec![PathPart::Field("name".into())],
            kind: SchemaErrorKind::WrongShape {
                expected: PayloadKind::Text,
                actual: PayloadKind::Integer,
            },
        },
    };
    let colour = Declaration::Malformed {
        declaration: ids[3],
        error: SchemaError {
            path: vec![PathPart::Field("colour".into())],
            kind: SchemaErrorKind::UnknownField,
        },
    };
    let alone = |declaration: &Declaration| match declaration {
    Declaration::Parsed {
        declaration,
        schema,
    } => TypeResolution::Declared {
        declaration: *declaration,
        schema: schema.clone(),
    },
    Declaration::Malformed { declaration, error } => TypeResolution::Malformed {
        declaration: *declaration,
        error: error.clone(),
    },
    };
    let ambiguous = TypeResolution::Ambiguous {
        claimants: vec![parsed(0, "A"), colour.clone()],
    };
    let cases = [
        (0, vec![], vec![], vec![("A", TypeResolution::Missing)]),
        (1, vec!["A"], vec![], vec![("A", alone(&parsed(0, "A")))]),
        (
            2,
            vec!["A"],
            vec![nameless.clone()],
            vec![("A", alone(&parsed(0, "A"))), ("", TypeResolution::Missing)],
        ),
        (
            3,
            vec!["", "A"],
            vec![nameless.clone()],
            vec![("", alone(&parsed(2, "")))],
        ),
        (
            4,
            vec!["", "A"],
            vec![nameless.clone()],
            vec![("A", ambiguous), ("", alone(&parsed(2, "")))],
        ),
        (
            5,
            vec!["", "A", "B"],
            vec![nameless],
            vec![
                ("A", alone(&colour)),
                ("B", alone(&parsed(4, "B"))),
                ("Type", TypeResolution::Builtin),
            ],
        ),
    ];
    for (end, names, without_name, resolutions) in cases {
        let view = graph.view().prefix(end).unwrap();
        let index = view.declarations();
        assert_eq!(index.names().collect::<Vec<_>>(), names, "boundary {end}");
        assert_eq!(
            index.nameless().cloned().collect::<Vec<_>>(),
            without_name,
            "boundary {end}"
        );
        for (name, resolution) in resolutions {
            assert_eq!(index.resolve(name), resolution, "{name:?} at {end}");
            assert_eq!(view.resolve_type(name), resolution, "{name:?} at {end}");
            assert_eq!(
                index.declared(name),
                match &resolution {
                TypeResolution::Declared {
                    declaration,
                    schema,
                } => Some((*declaration, schema)),
                _ => None,
                },
                "{name:?} at {end}"
            );
            assert_eq!(
                index.is_missing(name),
                resolution == TypeResolution::Missing,
                "{name:?} at {end}"
            );
        }
    }
    // Current declarations in append order, each with its claim.
    let index = graph.view().declarations();
    assert_eq!(
        index
            .iter()
            .map(|(name, declaration)| (name, declaration.id()))
            .collect::<Vec<_>>(),
        [
            (None, ids[1]),
            (Some(""), ids[2]),
            (Some("A"), ids[3]),
            (Some("B"), ids[4]),
        ]
    );
}
