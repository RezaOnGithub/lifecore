//! TypeCheck analyzer regression tests.

use std::marker::PhantomData;

use lifecore::{
    Body, Context, DiagnosticLocation, EntryContext, EntryIndex, LifeCore, LifeCoreList,
    OrderedSet, Payload, PayloadKind,
    analysis::{AnalysisRunner, Analyzer, AnalyzerFactory},
    serialize::{LifeCoreSerializeExt, NodeBody},
    typing::{FieldStatus, Type, ValueType},
};

use crate::{TypeCheck, TypeCheckFactory, TypeCheckMessage};

fn analyzer() -> TypeCheck<LifeCoreList> {
    TypeCheck {
        config: "test2".to_string(),
        _lc: PhantomData,
    }
}

fn analyze_at(lifecore: &LifeCoreList, window: Context) -> Vec<TypeCheckMessage> {
    analyzer().analyze_inner(lifecore, window).0
}

fn analyze_final(lifecore: &LifeCoreList) -> Vec<TypeCheckMessage> {
    analyze_at(
        lifecore,
        lifecore.last().unwrap().shrink_to_inclusive_left(),
    )
}

fn runner() -> AnalysisRunner<LifeCoreList> {
    let mut runner = AnalysisRunner::default();
    runner
        .register_analyzer_kind(Box::new(TypeCheckFactory::<LifeCoreList>::default()))
        .unwrap();
    runner
}

fn configure(lifecore: &mut LifeCoreList) {
    lifecore.add_analyzer(TypeCheckFactory::<LifeCoreList>::name(), "test2");
}

fn add_bootstrap(lifecore: &mut LifeCoreList) -> EntryContext {
    lifecore
        .add_typed_mock(Type::describe_self(), None)
        .unwrap()
        .e
}

fn add_type(lifecore: &mut LifeCoreList, ty: Type) -> EntryContext {
    lifecore.add_typed_mock(ty, None).unwrap().e
}

fn contains_message(
    messages: &[TypeCheckMessage],
    predicate: impl Fn(&TypeCheckMessage) -> bool,
) -> bool {
    messages.iter().any(predicate)
}

fn is_field_message(message: &TypeCheckMessage) -> bool {
    matches!(
        message,
        TypeCheckMessage::ElidedMandatoryField { .. }
            | TypeCheckMessage::ElidedEncouragedField { .. }
            | TypeCheckMessage::DeprecatedField { .. }
            | TypeCheckMessage::FieldTypeMismatch { .. }
            | TypeCheckMessage::ExtraneousField { .. }
    )
}

#[test]
fn initialize_suggests_the_bootstrap_type_when_missing() {
    let lifecore = LifeCoreList::default();

    let messages = analyzer().initialize_inner(&lifecore).0;

    assert_eq!(
        messages,
        vec![TypeCheckMessage::AddBootstrapNode {
            context: EntryIndex(0).before(),
        }]
    );
}

#[test]
fn initialize_accepts_the_bootstrap_type_when_present() {
    let mut lifecore = LifeCoreList::default();
    add_bootstrap(&mut lifecore);

    let messages = analyzer().initialize_inner(&lifecore).0;

    assert!(messages.is_empty());
}

#[test]
fn bootstrap_suggestion_uses_the_bounded_current_context() {
    let mut lifecore = LifeCoreList::default();
    configure(&mut lifecore);

    let output = runner().run(&lifecore).unwrap();
    let suggestion = output
        .suggestions()
        .into_iter()
        .find(|message| message.message.contains("bootstrap"))
        .unwrap();

    assert_eq!(
        suggestion.offending_event,
        DiagnosticLocation::Context(
            lifecore
                .last()
                .map_or_else(|| EntryIndex(0).before(), EntryContext::window)
        )
    );
}

#[test]
fn malformed_type_body_is_an_inconsistency_without_panicking() {
    let mut lifecore = LifeCoreList::default();
    let malformed = lifecore
        .add_mock(
            Type::TYPE_NAME,
            Body::from([("name", Payload::Number(1))]),
            None,
        )
        .unwrap();

    let messages = analyze_final(&lifecore);

    assert!(contains_message(&messages, |message| {
        matches!(
            message,
            TypeCheckMessage::MalformedNodeTypeBody { offending_event }
                if *offending_event == malformed.e
        )
    }));
}

#[test]
fn missing_type_declaration_is_reported_on_the_node() {
    let mut lifecore = LifeCoreList::default();
    let node = lifecore.add_mock("Missing", Body::default(), None).unwrap();

    let messages = analyze_final(&lifecore);

    assert!(contains_message(&messages, |message| {
        matches!(
            message,
            TypeCheckMessage::NodeTypeNonexistant {
                offending_event,
                ty_name,
            } if *offending_event == node.e && ty_name == "Missing"
        )
    }));
}

#[test]
fn duplicate_type_definition_points_at_the_newer_declaration() {
    let mut lifecore = LifeCoreList::default();
    add_bootstrap(&mut lifecore);
    add_type(&mut lifecore, Type::new("Thing", &[], &[], &[], &[]));
    let newer = add_type(
        &mut lifecore,
        Type::new(
            "Thing",
            &[("field", (ValueType::Text, FieldStatus::Optional))],
            &[],
            &[],
            &[],
        ),
    );

    let messages = analyze_final(&lifecore);

    assert!(contains_message(&messages, |message| {
        matches!(
            message,
            TypeCheckMessage::DuplicateTypeDefinition {
                offending_event,
                name,
            } if *offending_event == newer && name == "Thing"
        )
    }));
}

#[test]
fn replacing_a_type_through_its_parent_is_not_a_duplicate() {
    let mut lifecore = LifeCoreList::default();
    add_bootstrap(&mut lifecore);
    let older = add_type(&mut lifecore, Type::new("Thing", &[], &[], &[], &[]));
    lifecore
        .add_typed_mock(
            Type::new(
                "Thing",
                &[("field", (ValueType::Text, FieldStatus::Optional))],
                &[],
                &[],
                &[],
            ),
            Some(older),
        )
        .unwrap();

    let messages = analyze_final(&lifecore);

    assert!(!contains_message(&messages, |message| {
        matches!(
            message,
            TypeCheckMessage::DuplicateTypeDefinition { name, .. } if name == "Thing"
        )
    }));
}

#[test]
fn unresolved_additional_override_type_remains_a_warning_message() {
    let mut lifecore = LifeCoreList::default();
    add_bootstrap(&mut lifecore);
    let declaration = add_type(
        &mut lifecore,
        Type::new("Parent", &[], &[], &[], &["MissingChild"]),
    );

    let messages = analyze_final(&lifecore);

    assert!(contains_message(&messages, |message| {
        matches!(
            message,
            TypeCheckMessage::NonexistantOverrideType {
                offending_event,
                base_ty_name,
                override_ty_name,
                ..
            } if *offending_event == declaration
                && base_ty_name == "Parent"
                && override_ty_name == "MissingChild"
        )
    }));
}

#[test]
fn disallowed_node_override_is_reported_at_the_child() {
    let mut lifecore = LifeCoreList::default();
    add_bootstrap(&mut lifecore);
    add_type(&mut lifecore, Type::new("Parent", &[], &[], &[], &[]));
    add_type(&mut lifecore, Type::new("Child", &[], &[], &[], &[]));
    let parent = lifecore.add_mock("Parent", Body::default(), None).unwrap();
    let child = lifecore
        .add_mock("Child", Body::default(), Some(parent.e))
        .unwrap();

    let messages = analyze_final(&lifecore);

    assert!(contains_message(&messages, |message| {
        matches!(
            message,
            TypeCheckMessage::BadOverride {
                offending_event,
                parent_ty_name,
                child_ty_name,
            } if *offending_event == child.e
                && parent_ty_name == "Parent"
                && child_ty_name == "Child"
        )
    }));
}

#[test]
fn declared_additional_node_override_is_accepted() {
    let mut lifecore = LifeCoreList::default();
    add_bootstrap(&mut lifecore);
    add_type(
        &mut lifecore,
        Type::new("Parent", &[], &[], &[], &["Child"]),
    );
    add_type(&mut lifecore, Type::new("Child", &[], &[], &[], &[]));
    let parent = lifecore.add_mock("Parent", Body::default(), None).unwrap();
    lifecore
        .add_mock("Child", Body::default(), Some(parent.e))
        .unwrap();

    let messages = analyze_final(&lifecore);

    assert!(!contains_message(&messages, |message| {
        matches!(message, TypeCheckMessage::BadOverride { .. })
    }));
}

#[test]
fn field_statuses_and_extraneous_fields_are_enforced() {
    let mut lifecore = LifeCoreList::default();
    add_bootstrap(&mut lifecore);
    add_type(
        &mut lifecore,
        Type::new(
            "Task",
            &[
                ("must", (ValueType::Text, FieldStatus::Mandatory)),
                ("should", (ValueType::Text, FieldStatus::Encouraged)),
                ("old", (ValueType::Text, FieldStatus::Deprecated)),
                ("maybe", (ValueType::Number, FieldStatus::Optional)),
            ],
            &[],
            &[],
            &[],
        ),
    );
    let task = lifecore
        .add_mock(
            "Task",
            Body::from([
                ("old", Payload::text("legacy")),
                ("extra", Payload::text("nope")),
            ]),
            None,
        )
        .unwrap();

    let messages = analyze_final(&lifecore);

    assert!(contains_message(&messages, |message| {
        matches!(
            message,
            TypeCheckMessage::ElidedMandatoryField {
                offending_event,
                ty_name,
                field_name,
            } if *offending_event == task.e && ty_name == "Task" && field_name == "must"
        )
    }));
    assert!(contains_message(&messages, |message| {
        matches!(
            message,
            TypeCheckMessage::ElidedEncouragedField {
                offending_event,
                field_name,
                ..
            } if *offending_event == task.e && field_name == "should"
        )
    }));
    assert!(contains_message(&messages, |message| {
        matches!(
            message,
            TypeCheckMessage::DeprecatedField {
                offending_event,
                field_name,
                ..
            } if *offending_event == task.e && field_name == "old"
        )
    }));
    assert!(contains_message(&messages, |message| {
        matches!(
            message,
            TypeCheckMessage::ExtraneousField {
                offending_event,
                field_name,
                ..
            } if *offending_event == task.e && field_name == "extra"
        )
    }));
    assert!(!contains_message(&messages, |message| {
        matches!(
            message,
            TypeCheckMessage::ElidedMandatoryField { field_name, .. }
                | TypeCheckMessage::ElidedEncouragedField { field_name, .. }
                if field_name == "maybe"
        )
    }));
}

#[test]
fn empty_matching_collections_are_unfilled_for_mandatory_and_encouraged_fields() {
    let mut lifecore = LifeCoreList::default();
    add_bootstrap(&mut lifecore);
    add_type(
        &mut lifecore,
        Type::new(
            "CollectionPolicy",
            &[
                (
                    "mandatory_sequence",
                    (
                        ValueType::Sequence(Box::new(ValueType::Text)),
                        FieldStatus::Mandatory,
                    ),
                ),
                (
                    "mandatory_set",
                    (
                        ValueType::OrderedSet(Box::new(ValueType::Text)),
                        FieldStatus::Mandatory,
                    ),
                ),
                (
                    "encouraged_sequence",
                    (
                        ValueType::Sequence(Box::new(ValueType::Text)),
                        FieldStatus::Encouraged,
                    ),
                ),
                (
                    "encouraged_set",
                    (
                        ValueType::OrderedSet(Box::new(ValueType::Text)),
                        FieldStatus::Encouraged,
                    ),
                ),
                (
                    "optional_sequence",
                    (
                        ValueType::Sequence(Box::new(ValueType::Text)),
                        FieldStatus::Optional,
                    ),
                ),
                (
                    "deprecated_set",
                    (
                        ValueType::OrderedSet(Box::new(ValueType::Text)),
                        FieldStatus::Deprecated,
                    ),
                ),
            ],
            &[],
            &[],
            &[],
        ),
    );
    let node = lifecore
        .add_mock(
            "CollectionPolicy",
            Body::from([
                ("mandatory_sequence", Payload::Sequence(vec![])),
                ("mandatory_set", Payload::OrderedSet(OrderedSet::default())),
                ("encouraged_sequence", Payload::Sequence(vec![])),
                ("encouraged_set", Payload::OrderedSet(OrderedSet::default())),
                ("optional_sequence", Payload::Sequence(vec![])),
                ("deprecated_set", Payload::OrderedSet(OrderedSet::default())),
            ]),
            None,
        )
        .unwrap();

    let messages = analyze_final(&lifecore);
    let mandatory = messages
        .iter()
        .filter_map(|message| match message {
        TypeCheckMessage::ElidedMandatoryField {
            offending_event,
            field_name,
            ..
        } if *offending_event == node.e => Some(field_name.as_str()),
        _ => None,
        })
        .collect::<Vec<_>>();
    let encouraged = messages
        .iter()
        .filter_map(|message| match message {
        TypeCheckMessage::ElidedEncouragedField {
            offending_event,
            field_name,
            ..
        } if *offending_event == node.e => Some(field_name.as_str()),
        _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(mandatory, ["mandatory_sequence", "mandatory_set"]);
    assert_eq!(encouraged, ["encouraged_sequence", "encouraged_set"]);
    assert!(contains_message(&messages, |message| {
        matches!(
            message,
            TypeCheckMessage::DeprecatedField {
                offending_event,
                field_name,
                ..
            } if *offending_event == node.e && field_name == "deprecated_set"
        )
    }));
    assert!(!contains_message(&messages, |message| {
        matches!(
            message,
            TypeCheckMessage::ElidedMandatoryField { field_name, .. }
                | TypeCheckMessage::ElidedEncouragedField { field_name, .. }
                if field_name == "optional_sequence"
        )
    }));
    assert!(!contains_message(&messages, |message| {
        matches!(message, TypeCheckMessage::FieldTypeMismatch { offending_event, .. }
            if *offending_event == node.e)
    }));

    configure(&mut lifecore);
    let output = runner().run(&lifecore).unwrap();
    assert!(
        output
            .errors()
            .iter()
            .any(|message| message.message.contains("mandatory_sequence"))
    );
    assert!(
        output
            .warnings()
            .iter()
            .any(|message| message.message.contains("encouraged_sequence"))
    );
}

#[test]
fn empty_tuple_is_valid_for_a_mandatory_zero_arity_tuple() {
    let mut lifecore = LifeCoreList::default();
    add_bootstrap(&mut lifecore);
    add_type(
        &mut lifecore,
        Type::new(
            "EmptyTuple",
            &[("unit", (ValueType::Tuple(vec![]), FieldStatus::Mandatory))],
            &[],
            &[],
            &[],
        ),
    );
    let node = lifecore
        .add_mock(
            "EmptyTuple",
            Body::from([("unit", Payload::Sequence(vec![]))]),
            None,
        )
        .unwrap();

    let messages = analyze_final(&lifecore);

    assert!(!messages.iter().any(|message| is_field_message(message)
        && matches!(
            message,
            TypeCheckMessage::ElidedMandatoryField { offending_event, .. }
                | TypeCheckMessage::FieldTypeMismatch { offending_event, .. }
                if *offending_event == node.e
        )));
}

#[test]
fn incorrect_empty_container_is_only_a_type_mismatch() {
    let mut lifecore = LifeCoreList::default();
    add_bootstrap(&mut lifecore);
    let expected = ValueType::OrderedSet(Box::new(ValueType::Text));
    add_type(
        &mut lifecore,
        Type::new(
            "WrongContainer",
            &[("values", (expected.clone(), FieldStatus::Mandatory))],
            &[],
            &[],
            &[],
        ),
    );
    let node = lifecore
        .add_mock(
            "WrongContainer",
            Body::from([("values", Payload::Sequence(vec![]))]),
            None,
        )
        .unwrap();

    let messages = analyze_final(&lifecore);

    assert!(contains_message(&messages, |message| {
        matches!(
            message,
            TypeCheckMessage::FieldTypeMismatch {
                offending_event,
                field_name,
                expected_type,
                present_type: PayloadKind::Sequence,
                ..
            } if *offending_event == node.e
                && field_name == "values"
                && expected_type == &expected
        )
    }));
    assert!(!contains_message(&messages, |message| {
        matches!(
            message,
            TypeCheckMessage::ElidedMandatoryField {
                offending_event,
                field_name,
                ..
            } if *offending_event == node.e && field_name == "values"
        )
    }));
}

#[test]
fn rich_value_types_are_checked_recursively() {
    let mut lifecore = LifeCoreList::default();
    add_bootstrap(&mut lifecore);
    let status_type = ValueType::OneOf(vec![
        ValueType::TextLiteral("open".to_string()),
        ValueType::TextLiteral("closed".to_string()),
    ]);
    let tuple_type = ValueType::Tuple(vec![ValueType::Text, ValueType::Number]);
    let nested_type =
        ValueType::Sequence(Box::new(ValueType::OrderedSet(Box::new(ValueType::Number))));
    add_type(
        &mut lifecore,
        Type::new(
            "Rich",
            &[
                ("status", (status_type.clone(), FieldStatus::Optional)),
                ("tuple", (tuple_type.clone(), FieldStatus::Optional)),
                ("nested", (nested_type.clone(), FieldStatus::Optional)),
            ],
            &[],
            &[],
            &[],
        ),
    );
    let node = lifecore
        .add_mock(
            "Rich",
            Body::from([
                ("status", Payload::text("pending")),
                (
                    "tuple",
                    Payload::Sequence(vec![Payload::text("label"), Payload::text("wrong")]),
                ),
                (
                    "nested",
                    Payload::Sequence(vec![Payload::OrderedSet([Payload::text("wrong")].into())]),
                ),
            ]),
            None,
        )
        .unwrap();

    let messages = analyze_final(&lifecore);
    let mismatches = messages
        .iter()
        .filter_map(|message| match message {
        TypeCheckMessage::FieldTypeMismatch {
            offending_event,
            field_name,
            expected_type,
            present_type,
            ..
        } if *offending_event == node.e => Some((field_name.as_str(), expected_type, present_type)),
        _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(
        mismatches,
        [
            ("status", &status_type, &PayloadKind::Text),
            ("tuple", &tuple_type, &PayloadKind::Sequence),
            ("nested", &nested_type, &PayloadKind::Sequence),
        ]
    );

    configure(&mut lifecore);
    let output = runner().run(&lifecore).unwrap();
    assert!(output.errors().iter().any(|message| {
        message
            .message
            .contains(r#"expected OneOf("open", "closed"), got Text"#)
    }));
}

#[test]
fn valid_rich_value_types_do_not_emit_field_diagnostics() {
    let mut lifecore = LifeCoreList::default();
    add_bootstrap(&mut lifecore);
    add_type(
        &mut lifecore,
        Type::new(
            "Rich",
            &[
                (
                    "status",
                    (
                        ValueType::OneOf(vec![
                            ValueType::TextLiteral("open".to_string()),
                            ValueType::NumberLiteral(-1),
                        ]),
                        FieldStatus::Mandatory,
                    ),
                ),
                (
                    "nested",
                    (
                        ValueType::Sequence(Box::new(ValueType::Tuple(vec![
                            ValueType::Text,
                            ValueType::Number,
                        ]))),
                        FieldStatus::Optional,
                    ),
                ),
            ],
            &[],
            &[],
            &[],
        ),
    );
    let node = lifecore
        .add_mock(
            "Rich",
            Body::from([
                ("status", Payload::Number(-1)),
                (
                    "nested",
                    Payload::Sequence(vec![Payload::Sequence(vec![
                        Payload::text("answer"),
                        Payload::Number(42),
                    ])]),
                ),
            ]),
            None,
        )
        .unwrap();

    let messages = analyze_final(&lifecore);

    assert!(!messages.iter().any(|message| {
        is_field_message(message)
            && match message {
            TypeCheckMessage::ElidedMandatoryField {
                offending_event, ..
            }
            | TypeCheckMessage::ElidedEncouragedField {
                offending_event, ..
            }
            | TypeCheckMessage::DeprecatedField {
                offending_event, ..
            }
            | TypeCheckMessage::FieldTypeMismatch {
                offending_event, ..
            }
            | TypeCheckMessage::ExtraneousField {
                offending_event, ..
            } => *offending_event == node.e,
            _ => false,
            }
    }));
}

#[test]
fn fresh_old_node_is_rechecked_against_replacement_type() {
    let mut lifecore = LifeCoreList::default();
    add_bootstrap(&mut lifecore);
    let old_type = add_type(
        &mut lifecore,
        Type::new(
            "Task",
            &[("old", (ValueType::Text, FieldStatus::Mandatory))],
            &[],
            &[],
            &[],
        ),
    );
    let task = lifecore
        .add_mock(
            "Task",
            Body::from([("old", Payload::text("valid then"))]),
            None,
        )
        .unwrap();
    let before_replacement = task.e.shrink_to_inclusive_left();
    lifecore
        .add_typed_mock(
            Type::new(
                "Task",
                &[("new", (ValueType::Number, FieldStatus::Mandatory))],
                &[],
                &[],
                &[],
            ),
            Some(old_type),
        )
        .unwrap();

    let old_messages = analyze_at(&lifecore, before_replacement);
    let new_messages = analyze_final(&lifecore);

    assert!(!old_messages.iter().any(|message| {
        is_field_message(message)
            && matches!(
                message,
                TypeCheckMessage::ElidedMandatoryField { offending_event, .. }
                    | TypeCheckMessage::ExtraneousField { offending_event, .. }
                    if *offending_event == task.e
            )
    }));
    assert!(contains_message(&new_messages, |message| {
        matches!(
            message,
            TypeCheckMessage::ElidedMandatoryField {
                offending_event,
                field_name,
                ..
            } if offending_event.absolute() == task.e.absolute() && field_name == "new"
        )
    }));
    assert!(contains_message(&new_messages, |message| {
        matches!(
            message,
            TypeCheckMessage::ExtraneousField {
                offending_event,
                field_name,
                ..
            } if offending_event.absolute() == task.e.absolute() && field_name == "old"
        )
    }));
}

#[test]
fn stale_node_is_not_rechecked_against_replacement_type() {
    let mut lifecore = LifeCoreList::default();
    add_bootstrap(&mut lifecore);
    let old_type = add_type(
        &mut lifecore,
        Type::new(
            "Task",
            &[("old", (ValueType::Text, FieldStatus::Mandatory))],
            &[],
            &[],
            &[],
        ),
    );
    let old_task = lifecore
        .add_mock(
            "Task",
            Body::from([("old", Payload::text("valid then"))]),
            None,
        )
        .unwrap();
    lifecore
        .add_typed_mock(
            Type::new(
                "Task",
                &[("new", (ValueType::Number, FieldStatus::Mandatory))],
                &[],
                &[],
                &[],
            ),
            Some(old_type),
        )
        .unwrap();
    lifecore
        .add_mock(
            "Task",
            Body::from([("new", Payload::Number(1))]),
            Some(old_task.e),
        )
        .unwrap();

    let messages = analyze_final(&lifecore);

    assert!(!messages.iter().any(|message| {
        is_field_message(message)
            && match message {
            TypeCheckMessage::ElidedMandatoryField {
                offending_event, ..
            }
            | TypeCheckMessage::ElidedEncouragedField {
                offending_event, ..
            }
            | TypeCheckMessage::DeprecatedField {
                offending_event, ..
            }
            | TypeCheckMessage::FieldTypeMismatch {
                offending_event, ..
            }
            | TypeCheckMessage::ExtraneousField {
                offending_event, ..
            } => offending_event.absolute() == old_task.e.absolute(),
            _ => false,
            }
    }));
}

#[test]
fn valid_typed_edge_is_accepted_across_all_historical_prefixes() {
    let mut lifecore = LifeCoreList::default();
    add_bootstrap(&mut lifecore);
    add_type(&mut lifecore, Type::new("From", &[], &["link"], &[], &[]));
    add_type(&mut lifecore, Type::new("To", &[], &[], &["link"], &[]));
    let from = lifecore.add_mock("From", Body::default(), None).unwrap();
    let to = lifecore.add_mock("To", Body::default(), None).unwrap();
    lifecore.connect_mock(from.e, "link", to.e).unwrap();
    configure(&mut lifecore);

    let output = runner().run(&lifecore).unwrap();

    assert!(!output.has_error(), "unexpected output: {output:?}");
}

#[test]
fn disallowed_outgoing_edge_identifies_the_head_node() {
    let mut lifecore = LifeCoreList::default();
    add_bootstrap(&mut lifecore);
    add_type(&mut lifecore, Type::new("From", &[], &[], &[], &[]));
    add_type(&mut lifecore, Type::new("To", &[], &[], &["link"], &[]));
    let from = lifecore.add_mock("From", Body::default(), None).unwrap();
    let to = lifecore.add_mock("To", Body::default(), None).unwrap();
    let edge = lifecore.connect_mock(from.e, "link", to.e).unwrap();

    let messages = analyze_final(&lifecore);

    assert!(contains_message(&messages, |message| {
        matches!(
            message,
            TypeCheckMessage::BadEdgeEndpoint {
                offending_event,
                edge_ty_name,
                bad_node_ty_name,
                bad_node_is_head: true,
            } if *offending_event == edge.e
                && edge_ty_name == "link"
                && bad_node_ty_name == "From"
        )
    }));
}

#[test]
fn disallowed_incoming_edge_identifies_the_tail_node() {
    let mut lifecore = LifeCoreList::default();
    add_bootstrap(&mut lifecore);
    add_type(&mut lifecore, Type::new("From", &[], &["link"], &[], &[]));
    add_type(&mut lifecore, Type::new("To", &[], &[], &[], &[]));
    let from = lifecore.add_mock("From", Body::default(), None).unwrap();
    let to = lifecore.add_mock("To", Body::default(), None).unwrap();
    let edge = lifecore.connect_mock(from.e, "link", to.e).unwrap();

    let messages = analyze_final(&lifecore);

    assert!(contains_message(&messages, |message| {
        matches!(
            message,
            TypeCheckMessage::BadEdgeEndpoint {
                offending_event,
                edge_ty_name,
                bad_node_ty_name,
                bad_node_is_head: false,
            } if *offending_event == edge.e
                && edge_ty_name == "link"
                && bad_node_ty_name == "To"
        )
    }));
}

#[test]
fn factory_reports_its_stable_registered_name() {
    let factory = TypeCheckFactory::<LifeCoreList>::default();

    assert_eq!(factory.analyzer_name(), "Type_Check");
}
