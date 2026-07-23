// use lifecore::{
//     DiagnosticLocation, EventContext, LifeCore, LifeCoreList, Body, NodeType, NodeTypeClass,
//     PointedEventContext, TypedBody,
//     analysis::{AnalysisRunner, AnalyzerFactory, Message},
// };

// use crate::TypeCheckFactory;

// fn runner() -> AnalysisRunner<LifeCoreList> {
//     let mut runner = AnalysisRunner::default();
//     runner
//         .register_analyzer_kind(Box::new(TypeCheckFactory::<LifeCoreList>::default()))
//         .unwrap();
//     runner
// }

// fn configure(lc: &mut LifeCoreList) {
//     lc.add_analyzer(TypeCheckFactory::<LifeCoreList>::name(), "test2");
// }

// fn add_bootstrap(lc: &mut LifeCoreList) {
//     lc.add(NodeType::name(), NodeType::describe_self().into(), None)
//         .unwrap();
// }

// fn invalid_override_graph() -> (LifeCoreList, PointedEventContext, PointedEventContext) {
//     let mut lc = LifeCoreList::default();
//     add_bootstrap(&mut lc);
//     lc.add(
//         NodeType::name(),
//         NodeType::new("Parent", &[], &[], &[], &[]).into(),
//         None,
//     )
//     .unwrap();
//     lc.add(
//         NodeType::name(),
//         NodeType::new("Child", &[], &[], &[], &[]).into(),
//         None,
//     )
//     .unwrap();
//     let parent = lc.add("Parent", Body::default(), None).unwrap();
//     let child = lc
//         .add("Child", Body::default(), Some(parent.e))
//         .unwrap();
//     (lc, parent.e, child.e)
// }

// #[test]
// fn runner_analyzes_a_valid_typed_edge_without_panicking() {
//     let mut lc = LifeCoreList::default();
//     add_bootstrap(&mut lc);
//     lc.add(
//         NodeType::name(),
//         NodeType::new("From", &[], &["link"], &[], &[]).into(),
//         None,
//     )
//     .unwrap();
//     lc.add(
//         NodeType::name(),
//         NodeType::new("To", &[], &[], &["link"], &[]).into(),
//         None,
//     )
//     .unwrap();
//     let from = lc.add("From", Body::default(), None).unwrap();
//     let to = lc.add("To", Body::default(), None).unwrap();
//     lc.connect(from.e, "link", to.e).unwrap();
//     configure(&mut lc);

//     let output = runner().run(&lc).unwrap();

//     assert!(!output.has_error(), "unexpected output: {output:?}");
// }

// #[test]
// fn bad_override_is_reported_once_at_the_child_event() {
//     let (mut lc, _, child) = invalid_override_graph();
//     configure(&mut lc);

//     let output = runner().run(&lc).unwrap();
//     let bad_overrides = output
//         .0
//         .iter()
//         .filter_map(|message| match message {
//         Message::Inconsistency(message) if message.message.contains("does not allow") => {
//             Some(message)
//         }
//         _ => None,
//         })
//         .collect::<Vec<_>>();

//     assert_eq!(bad_overrides.len(), 1);
//     assert_eq!(bad_overrides[0].offending_event, child.into());
// }

// #[test]
// fn duplicate_type_definition_points_at_the_newer_declaration() {
//     let mut lc = LifeCoreList::default();
//     add_bootstrap(&mut lc);
//     lc.add(
//         NodeType::name(),
//         NodeType::new("Thing", &[], &[], &[], &[]).into(),
//         None,
//     )
//     .unwrap();
//     let newer = lc
//         .add(
//             NodeType::name(),
//             NodeType::new(
//                 "Thing",
//                 &[(
//                     "field",
//                     (
//                         lifecore::PayloadType::Text,
//                         lifecore::NodeTypeFieldStatus::Optional,
//                     ),
//                 )],
//                 &[],
//                 &[],
//                 &[],
//             )
//             .into(),
//             None,
//         )
//         .unwrap();
//     configure(&mut lc);

//     let output = runner().run(&lc).unwrap();
//     let duplicates = output
//         .0
//         .iter()
//         .filter_map(|message| match message {
//         Message::Inconsistency(message) if message.message.contains("defined multiple times") => {
//             Some(message)
//         }
//         _ => None,
//         })
//         .collect::<Vec<_>>();

//     assert_eq!(duplicates.len(), 1);
//     assert_eq!(duplicates[0].offending_event, newer.e.into());
// }

// #[test]
// fn bootstrap_suggestion_has_a_universe_window_location() {
//     let mut lc = LifeCoreList::default();
//     configure(&mut lc);

//     let output = runner().run(&lc).unwrap();
//     let suggestion = output
//         .0
//         .iter()
//         .find_map(|message| match message {
//         Message::Suggestion(message) if message.message.contains("bootstrap") => Some(message),
//         _ => None,
//         })
//         .unwrap();

//     assert_eq!(
//         suggestion.offending_event,
//         DiagnosticLocation::Window(EventContext::universe())
//     );
// }

// // TODO revise
// #[cfg(never)]
// #[test]
// fn override_analysis_does_not_panic_when_the_parent_precedes_the_window() {
//     let (lc, _, child) = invalid_override_graph();
//     let child_only_window = child.inclusive_right();
//     let mut factory = TypeCheckFactory::<LifeCoreList>::default();
//     let analyzer = factory.analyzer_with_config("test2").unwrap();

//     let _ = analyzer.analyze(&lc, child_only_window);
// }

// // TODO revise
// #[cfg(never)]
// #[test]
// fn edge_node_requires_both_an_incoming_and_an_outgoing_edge() {
//     let mut lc = LifeCoreList::default();
//     add_bootstrap(&mut lc);
//     let mut edge_node_type = NodeType::new("Edge_Like", &[], &["link"], &[], &[]);
//     edge_node_type.class = NodeTypeClass::EdgeNode;
//     lc.add(NodeType::name(), edge_node_type.into(), None)
//         .unwrap();
//     lc.add(
//         NodeType::name(),
//         NodeType::new("Target", &[], &[], &["link"], &[]).into(),
//         None,
//     )
//     .unwrap();
//     let edge_node = lc.add("Edge_Like", Body::default(), None).unwrap();
//     let target = lc.add("Target", Body::default(), None).unwrap();
//     lc.connect(edge_node.e, "link", target.e).unwrap();
//     let final_window = lc.last().unwrap().inclusive_left_items();
//     let mut factory = TypeCheckFactory::<LifeCoreList>::default();
//     let analyzer = factory.analyzer_with_config("test2").unwrap();

//     let output = analyzer.analyze(&lc, final_window);

//     assert!(output.0.iter().any(|message| {
//         matches!(message, Message::Inconsistency(message)
//             if message.message.contains("Node class EdgeNode of type Edge_Like"))
//     }));
// }

use crate::{FieldDecl, FieldStatus, ValueType};

#[test]
fn value_type_parser_and_display_round_trip_recursive_types() {
    let values = [
        ValueType::PreciseTime,
        ValueType::Sequence(Box::new(ValueType::OneOf(vec![
            ValueType::Text,
            ValueType::Number,
        ]))),
        ValueType::Tuple(vec![
            ValueType::OrderedSet(Box::new(ValueType::Text)),
            ValueType::Sequence(Box::new(ValueType::Tuple(vec![
                ValueType::Number,
                ValueType::PreciseTime,
            ]))),
        ]),
    ];

    for value in values {
        let displayed = value.to_string();
        assert_eq!(displayed.parse::<ValueType>().unwrap(), value);
    }
}

#[test]
fn value_type_parser_accepts_lowercase_or_canonical_keywords() {
    let parsed = "sequence(oneof(text, OrderedSet(number)))"
        .parse::<ValueType>()
        .unwrap();

    assert_eq!(
        parsed,
        ValueType::Sequence(Box::new(ValueType::OneOf(vec![
            ValueType::Text,
            ValueType::OrderedSet(Box::new(ValueType::Number)),
        ])))
    );
    assert_eq!(
        parsed.to_string(),
        "Sequence(OneOf(Text, OrderedSet(Number)))"
    );
}

#[test]
fn field_decl_parser_and_display_round_trip() {
    let parsed = " tags = sequence(oneof(text, Number)) : mandatory "
        .parse::<FieldDecl>()
        .unwrap();

    assert_eq!(parsed.name, "tags");
    assert_eq!(parsed.status, FieldStatus::Mandatory);
    assert_eq!(
        parsed.ty,
        ValueType::Sequence(Box::new(ValueType::OneOf(vec![
            ValueType::Text,
            ValueType::Number,
        ])))
    );
    assert_eq!(
        parsed.to_string(),
        "tags = Sequence(OneOf(Text, Number)) : Mandatory"
    );
    assert_eq!(parsed.to_string().parse::<FieldDecl>().unwrap(), parsed);
}

#[test]
fn field_decl_parser_accepts_each_status_spelling() {
    for (keyword, expected) in [
        ("Mandatory", FieldStatus::Mandatory),
        ("encouraged", FieldStatus::Encouraged),
        ("Deprecated", FieldStatus::Deprecated),
        ("optional", FieldStatus::Optional),
    ] {
        let parsed = format!("field_name = Text : {keyword}")
            .parse::<FieldDecl>()
            .unwrap();
        assert_eq!(parsed.status, expected);
    }
}

#[test]
fn type_declaration_parsers_reject_bad_names_keywords_and_grammar() {
    for input in [
        "TEXT",
        "tExT",
        "Sequence(Text, Number)",
        "OneOf(Text,)",
        "Tuple(Text Number)",
    ] {
        assert!(
            input.parse::<ValueType>().is_err(),
            "{input:?} unexpectedly parsed"
        );
    }

    for input in [
        "Tags = Text : Mandatory",
        "_tags = Text : Mandatory",
        "tagName = Text : Mandatory",
        "tags = Text : MANDATORY",
        "tags = Text : mAndatory",
        "tags = Sequence(Text, Number) : Mandatory",
        "tags Text : Mandatory",
        "tags = Text Mandatory",
    ] {
        assert!(
            input.parse::<FieldDecl>().is_err(),
            "{input:?} unexpectedly parsed"
        );
    }
}
