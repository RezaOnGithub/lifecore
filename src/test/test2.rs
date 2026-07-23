//! Tests written and maintained by Agents

use crate::{FieldQuery, FieldQueryPathComponent};

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use crate::{
    Body, Edge, EntryContext, LifeCore, LifeCoreList, Node,
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
    let child = lc.add_mock("Child", Body::default(), Some(root.e)).unwrap();
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

use crate::{
    OrderedSet, PayloadKind, Timestamp,
    serialize::{Error as SerializeError, LifeCoreSerializeExt, NodeBody, NodeField},
    typing::{FieldDecl, FieldStatus, Type, ValueType},
};

#[test]
fn body_facade_supports_map_operations_and_convertible_collection_building() {
    let mut body = Body::new();
    assert!(body.is_empty());
    assert_eq!(body.insert("count", Payload::Number(1)), None);
    assert!(body.contains_key("count"));
    assert_eq!(body.get("count"), Some(&Payload::Number(1)));
    *body.get_mut("count").unwrap() = Payload::Number(2);
    assert_eq!(body.values().collect::<Vec<_>>(), vec![&Payload::Number(2)]);
    assert_eq!(body.remove("count"), Some(Payload::Number(2)));

    let body = Body::from([("title", Payload::text("Task"))]);
    assert_eq!(
        body.keys().map(String::as_str).collect::<Vec<_>>(),
        ["title"]
    );
}

#[test]
fn ordered_set_facade_preserves_order_without_exposing_its_backing_collection() {
    let mut values = OrderedSet::from(["first".to_string(), "second".to_string()]);
    assert_eq!(values.first().map(String::as_str), Some("first"));
    assert_eq!(values.last().map(String::as_str), Some("second"));
    assert_eq!(values.get(1).map(String::as_str), Some("second"));
    assert!(!values.insert("second".to_string()));
    assert!(values.remove(&"first".to_string()));
    values.extend(["third".to_string(), "fourth".to_string()]);
    values.retain(|value| value != "third");

    assert_eq!(values.into_iter().collect::<Vec<_>>(), ["second", "fourth"]);
}

#[test]
fn value_type_syntax_and_payload_checks_keep_the_existing_wire_language() {
    let value_type =
        r#"Sequence(Tuple(Text, OneOf("ready: now", -1)))"#.parse::<ValueType>().unwrap();
    assert_eq!(
        value_type.to_string(),
        r#"Sequence(Tuple(Text, OneOf("ready: now", -1)))"#
    );
    assert!(
        value_type
            .check(&Payload::Sequence(vec![Payload::Sequence(vec![
                Payload::text("task"),
                Payload::text("ready: now"),
            ])]))
            .is_ok()
    );
    assert!(matches!(
        value_type.check(&Payload::Number(1)),
        Err(SerializeError::UnexpectedPayloadKind {
            actual: PayloadKind::Number,
            ..
        })
    ));
}

#[test]
fn field_declaration_uses_its_three_element_payload_tuple_and_accessors() {
    let field = r#"status = OneOf("ready", "done", -1) : Encouraged"#
        .parse::<FieldDecl>()
        .unwrap();
    let payload = field.clone().into_payload();

    assert_eq!(field.name(), "status");
    assert_eq!(field.status(), FieldStatus::Encouraged);
    assert_eq!(
        payload,
        Payload::Sequence(vec![
            Payload::text("status"),
            Payload::text(r#"OneOf("ready", "done", -1)"#),
            Payload::text("Encouraged"),
        ])
    );
    assert_eq!(FieldDecl::try_from_payload(&payload).unwrap(), field);
    assert!(FieldDecl::try_from_payload(&Payload::text("legacy")).is_err());

    assert_eq!(
        FieldStatus::try_from_payload(&Payload::text("mandatory")).unwrap(),
        FieldStatus::Mandatory
    );
    assert_eq!(
        FieldStatus::Mandatory.into_payload(),
        Payload::text("Mandatory")
    );
}

#[test]
fn node_field_round_trips_primitives_and_nested_ordered_sets() {
    let values = OrderedSet::from([OrderedSet::from([1_i64, 2_i64]), OrderedSet::from([3_i64])]);
    let payload = values.clone().into_payload();

    assert_eq!(
        <OrderedSet<OrderedSet<i64>>>::try_from_payload(&payload).unwrap(),
        values
    );
    assert!(matches!(
        OrderedSet::<i64>::try_from_payload(&Payload::text("not a set")),
        Err(SerializeError::UnexpectedPayloadKind { .. })
    ));
    assert!(matches!(
        OrderedSet::<i64>::try_from_payload(&Payload::OrderedSet(
            [Payload::Number(1), Payload::text("wrong")]
                .into_iter()
                .collect(),
        )),
        Err(SerializeError::InvalidCollectionElement { index: 1, .. })
    ));

    let strings = OrderedSet::from(["first".to_string(), "second".to_string()]);
    assert_eq!(
        OrderedSet::<String>::try_from_payload(&strings.clone().into_payload()).unwrap(),
        strings
    );

    let times = OrderedSet::from([
        "2026-07-23T00:00:00Z".parse::<Timestamp>().unwrap(),
        "2026-07-24T00:00:00Z".parse::<Timestamp>().unwrap(),
    ]);
    assert_eq!(
        OrderedSet::<Timestamp>::try_from_payload(&times.clone().into_payload()).unwrap(),
        times
    );
}

#[test]
fn stored_type_body_round_trips_and_defaults_absent_optional_sets() {
    let declaration = Type::new(
        "Task",
        &[("title", (ValueType::Text, FieldStatus::Mandatory))],
        &["blocks"],
        &["contains"],
        &["ArchivedTask"],
    );
    assert_eq!(
        Type::try_from_body(&declaration.clone().into_body()).unwrap(),
        declaration
    );

    let empty = Body::from([("name", Payload::text("Empty"))]);
    assert_eq!(
        Type::try_from_body(&empty).unwrap(),
        Type::new("Empty", &[], &[], &[], &[])
    );
    assert!(matches!(
        Type::try_from_body(&Body::default()),
        Err(SerializeError::MissingBodyField { field_name }) if field_name == "name"
    ));
    assert!(matches!(
        Type::try_from_body(&Body::from([
            ("name", Payload::text("Bad")),
            ("outgoing", Payload::Sequence(vec![])),
        ])),
        Err(SerializeError::InvalidBodyField { field_name, .. }) if field_name == "outgoing"
    ));
}

#[test]
fn type_syntax_rejects_invalid_values_without_broadening_the_language() {
    for input in [
        "TEXT",
        r#""unterminated"#,
        "Sequence(Text, Number)",
        "OneOf(Text,)",
        "9223372036854775808",
    ] {
        assert!(
            input.parse::<ValueType>().is_err(),
            "{input:?} unexpectedly parsed"
        );
    }
    for input in [
        "Tags = Text : Mandatory",
        "tagName = Text : Mandatory",
        "tags = Text : MANDATORY",
        "tags Text : Mandatory",
    ] {
        assert!(
            input.parse::<FieldDecl>().is_err(),
            "{input:?} unexpectedly parsed"
        );
    }
}

#[derive(Clone, PartialEq, Eq)]
struct TaskBody {
    title: String,
}

impl NodeBody for TaskBody {
    const TYPE_NAME: &'static str = "Task";

    fn describe_self() -> Type {
        Type::new(
            Self::TYPE_NAME,
            &[("title", (ValueType::Text, FieldStatus::Mandatory))],
            &[],
            &[],
            &[],
        )
    }

    fn into_body(self) -> Body {
        Body::from([("title", self.title.into_payload())])
    }

    fn try_from_body(value: &Body) -> Result<Self, SerializeError> {
        let title = value
            .get("title")
            .ok_or_else(|| SerializeError::MissingBodyField {
                field_name: "title".to_string(),
            })
            .and_then(String::try_from_payload)?;
        Ok(Self { title })
    }
}

#[test]
fn typed_mock_extension_works_without_expanding_the_lifecore_trait() {
    let mut lifecore = LifeCoreList::default();
    let added = lifecore
        .add_typed_mock(
            TaskBody {
                title: "Write tests".to_string(),
            },
            None,
        )
        .unwrap();

    assert_eq!(added.ty_name, TaskBody::TYPE_NAME);
    assert_eq!(
        TaskBody::try_from_body(&added.field).unwrap().title,
        "Write tests"
    );
}
