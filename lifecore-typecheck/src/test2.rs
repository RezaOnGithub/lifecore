use lifecore::{
    EventContext, LifeCore, LifeCoreList, NodeBodyExt, NodeType, NodeTypeClass, TypedBody,
    analysis::{AnalysisRunner, AnalyzerFactory, Message},
};

use crate::TypeCheckFactory;

fn runner() -> AnalysisRunner<LifeCoreList> {
    let mut runner = AnalysisRunner::default();
    runner
        .register_analyzer_kind(Box::new(TypeCheckFactory::<LifeCoreList>::default()))
        .unwrap();
    runner
}

fn configure(lc: &mut LifeCoreList) {
    lc.add_analyzer(TypeCheckFactory::<LifeCoreList>::name(), "test2");
}

fn add_bootstrap(lc: &mut LifeCoreList) {
    lc.add(NodeType::name(), NodeType::describe_self().into(), None)
        .unwrap();
}

fn invalid_override_graph() -> (LifeCoreList, EventContext, EventContext) {
    let mut lc = LifeCoreList::default();
    add_bootstrap(&mut lc);
    lc.add(
        NodeType::name(),
        NodeType::new("Parent", &[], &[], &[], &[]).into(),
        None,
    )
    .unwrap();
    lc.add(
        NodeType::name(),
        NodeType::new("Child", &[], &[], &[], &[]).into(),
        None,
    )
    .unwrap();
    let parent = lc.add("Parent", NodeBodyExt::default(), None).unwrap();
    let child = lc
        .add("Child", NodeBodyExt::default(), Some(parent.e))
        .unwrap();
    (lc, parent.e, child.e)
}

#[test]
fn runner_analyzes_a_valid_typed_edge_without_panicking() {
    let mut lc = LifeCoreList::default();
    add_bootstrap(&mut lc);
    lc.add(
        NodeType::name(),
        NodeType::new("From", &[], &["link"], &[], &[]).into(),
        None,
    )
    .unwrap();
    lc.add(
        NodeType::name(),
        NodeType::new("To", &[], &[], &["link"], &[]).into(),
        None,
    )
    .unwrap();
    let from = lc.add("From", NodeBodyExt::default(), None).unwrap();
    let to = lc.add("To", NodeBodyExt::default(), None).unwrap();
    lc.connect(from.e, "link", to.e).unwrap();
    configure(&mut lc);

    let output = runner().run(&lc).unwrap();

    assert!(!output.has_error(), "unexpected output: {output:?}");
}

#[test]
fn bad_override_is_reported_once_at_the_child_event() {
    let (mut lc, _, child) = invalid_override_graph();
    configure(&mut lc);

    let output = runner().run(&lc).unwrap();
    let bad_overrides = output
        .0
        .iter()
        .filter_map(|message| match message {
        Message::Inconsistency(message) if message.message.contains("does not allow") => {
            Some(message)
        }
        _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(bad_overrides.len(), 1);
    assert_eq!(bad_overrides[0].offending_event, child);
}

#[test]
fn duplicate_type_definition_points_at_the_newer_declaration() {
    let mut lc = LifeCoreList::default();
    add_bootstrap(&mut lc);
    lc.add(
        NodeType::name(),
        NodeType::new("Thing", &[], &[], &[], &[]).into(),
        None,
    )
    .unwrap();
    let newer = lc
        .add(
            NodeType::name(),
            NodeType::new(
                "Thing",
                &[(
                    "field",
                    (
                        lifecore::PayloadType::Text,
                        lifecore::NodeTypeFieldStatus::Optional,
                    ),
                )],
                &[],
                &[],
                &[],
            )
            .into(),
            None,
        )
        .unwrap();
    configure(&mut lc);

    let output = runner().run(&lc).unwrap();
    let duplicates = output
        .0
        .iter()
        .filter_map(|message| match message {
        Message::Inconsistency(message) if message.message.contains("defined multiple times") => {
            Some(message)
        }
        _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(duplicates.len(), 1);
    assert_eq!(duplicates[0].offending_event, newer.e);
}

// TODO revise
#[cfg(never)]
#[test]
fn override_analysis_does_not_panic_when_the_parent_precedes_the_window() {
    let (lc, _, child) = invalid_override_graph();
    let child_only_window = child.inclusive_right();
    let mut factory = TypeCheckFactory::<LifeCoreList>::default();
    let analyzer = factory.analyzer_with_config("test2").unwrap();

    let _ = analyzer.analyze(&lc, child_only_window);
}

// TODO revise
#[cfg(never)]
#[test]
fn edge_node_requires_both_an_incoming_and_an_outgoing_edge() {
    let mut lc = LifeCoreList::default();
    add_bootstrap(&mut lc);
    let mut edge_node_type = NodeType::new("Edge_Like", &[], &["link"], &[], &[]);
    edge_node_type.class = NodeTypeClass::EdgeNode;
    lc.add(NodeType::name(), edge_node_type.into(), None)
        .unwrap();
    lc.add(
        NodeType::name(),
        NodeType::new("Target", &[], &[], &["link"], &[]).into(),
        None,
    )
    .unwrap();
    let edge_node = lc.add("Edge_Like", NodeBodyExt::default(), None).unwrap();
    let target = lc.add("Target", NodeBodyExt::default(), None).unwrap();
    lc.connect(edge_node.e, "link", target.e).unwrap();
    let final_window = lc.last().unwrap().inclusive_left_items();
    let mut factory = TypeCheckFactory::<LifeCoreList>::default();
    let analyzer = factory.analyzer_with_config("test2").unwrap();

    let output = analyzer.analyze(&lc, final_window);

    assert!(output.0.iter().any(|message| {
        matches!(message, Message::Inconsistency(message)
            if message.message.contains("Node class EdgeNode of type Edge_Like"))
    }));
}
