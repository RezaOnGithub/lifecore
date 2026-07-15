use crate::{
    BasicValidationFailure,
    BasicValidationFailureScenario::{self, *},
    EventContext, LifeCore, LifeCoreError, LifeCoreList, NodeBodyExt, NodeType, NodeTypeClass,
    NodeTypeFieldStatus, PayloadExt, PayloadType, TypedBody, analysis,
};

fn text_body(value: &str) -> NodeBodyExt {
    NodeBodyExt::from(&[("date", PayloadExt::Text(value.to_string()))])
}

// TODO use NodeType::new
fn type_decl(
    name: &str,
    fields: &[(&str, (PayloadType, NodeTypeFieldStatus))],
    outgoing: &[&str],
    incoming: &[&str],
    additional_override: &[&str],
) -> NodeType {
    NodeType {
        name: name.to_string(),
        class: NodeTypeClass::Regular,
        outgoing: outgoing.iter().map(|x| x.to_string()).collect(),
        incoming: incoming.iter().map(|x| x.to_string()).collect(),
        additional_override: additional_override.iter().map(|x| x.to_string()).collect(),
        field: fields
            .iter()
            .map(|(name, value)| (name.to_string(), value.clone()))
            .collect(),
    }
}

fn generic_parent_child_override_scenario<LC: LifeCore>(mut lc: LC) {
    let parent = lc.add("", text_body("parent"), None).unwrap();
    let child = lc.add("", text_body("child"), Some(parent.e)).unwrap();
    let sibling_child = lc.add("", text_body("sibling"), Some(parent.e)).unwrap();

    // TODO revise generic tests a bit
    // assert!(lc.is_stale(lc.last().look_at(parent.e)));
    // assert!(!lc.is_stale(lc.last().look_at(child.e)));
    // assert!(!lc.is_stale(lc.last().look_at(sibling_child.e)));
    // assert!(lc.basic_validation().is_success());
    // assert_eq!(lc.total_event_count(), 3);
}

fn generic_last_on_empty_lifecore<LC: LifeCore>(lc: LC) {
    assert!(lc.last().is_none());
}

fn generic_get_with_unpointed_context_panics<LC: LifeCore>(lc: &LC) {
    let _ = lc.get(EventContext::universe());
}

fn generic_window_with_unpointed_context_panics<LC: LifeCore>(lc: &LC) {
    let _ = lc.window([EventContext::universe()]).count();
}

fn generic_node_on_edge_context<LC: LifeCore>(lc: &LC, edge: EventContext) {
    assert!(lc.node(edge).is_none());
}

fn generic_edge_on_node_context<LC: LifeCore>(lc: &LC, node: EventContext) {
    assert!(lc.edge(node).is_none())
}

fn generic_is_stale_with_out_of_range_context_panics<LC: LifeCore>(lc: &LC) {
    let _ = lc.is_stale(EventContext::universe().look_raw(usize::MAX / 2));
}

#[test]
fn new_is_valid() {
    let lc = LifeCoreList::new();
    assert!(lc.basic_validation().is_ok());
}

#[test]
fn basic_01() {
    let mut lc = LifeCoreList::new();
    let n0 = lc.add("", text_body("yellow"), None).unwrap();
    let n1 = lc.add("", text_body("orange"), None).unwrap();
    let _e0 = lc.connect(n0.e, "skrunkle", n1.e).unwrap();
    assert!(lc.basic_validation().is_ok());
}

#[test]
fn basic_02() {}

#[test]
fn no_dupe_node() {
    let mut lc = LifeCoreList::new();
    let _n0 = lc.add("", text_body("yellow"), None).unwrap();
    let error = lc.add("", text_body("yellow"), None).unwrap_err();
    assert!(matches!(
        error,
        LifeCoreError::BasicValidationFailure(BasicValidationFailure {
            reason: DuplicateNode,
            ..
        })
    ));
}

#[test]
fn no_dupe_edge() {
    let mut lc = LifeCoreList::new();
    let n0 = lc.add("", text_body("yellow"), None).unwrap();
    let n1 = lc.add("", text_body("orange"), None).unwrap();
    let _e0 = lc.connect(n0.e, "skrunkle", n1.e).unwrap();
    let error = lc.connect(n0.e, "skrunkle", n1.e).unwrap_err();
    assert!(matches!(
        error,
        LifeCoreError::BasicValidationFailure(BasicValidationFailure {
            reason: DuplicateEdge,
            ..
        })
    ));
}

#[test]
fn dupe_parent_and_child() {
    let mut lc = LifeCoreList::new();
    let n0 = lc.add("", text_body("yellow"), None).unwrap();
    let _n1 = lc.add("", text_body("yellow"), Some(n0.e)).unwrap();
    assert!(lc.basic_validation().is_ok());
}

#[test]
fn stale_rule_01() {
    let mut lc = LifeCoreList::new();
    let parent = lc.add("", text_body("yellow"), None).unwrap();
    let child = lc.add("", text_body("yellow"), Some(parent.e)).unwrap();
    let error = lc.connect(parent.e, "", child.e).unwrap_err();
    assert!(matches!(
        error,
        LifeCoreError::BasicValidationFailure(BasicValidationFailure {
            reason: StaleRuleViolation,
            ..
        })
    ));
}

#[test]
fn stale_rule_02() {
    let mut lc = LifeCoreList::new();
    let parent = lc.add("", text_body("yellow"), None).unwrap();
    let child = lc.add("", text_body("yellow"), Some(parent.e)).unwrap();
    let error = lc.connect(child.e, "", parent.e).unwrap_err();
    assert!(matches!(
        error,
        LifeCoreError::BasicValidationFailure(BasicValidationFailure {
            reason: StaleRuleViolation,
            ..
        })
    ));
}

#[test]
fn child_can_be_added_to_already_overridden_node() {
    generic_parent_child_override_scenario(LifeCoreList::new());
}

#[test]
fn families_include_root_and_children_after_overrides() {
    let mut lc = LifeCoreList::new();
    let root = lc.add("", text_body("root"), None).unwrap();
    let child = lc.add("", text_body("child"), Some(root.e)).unwrap();
    let second_child = lc.add("", text_body("second-child"), Some(root.e)).unwrap();

    let families = lc.non_solo_families(EventContext::universe());
    assert_eq!(families.len(), 1);
    assert_eq!(families[0].len(), 3);
    assert!(
        families[0]
            .iter()
            .any(|x| x.absolute() == root.e.absolute())
    );
    assert!(
        families[0]
            .iter()
            .any(|x| x.absolute() == child.e.absolute())
    );
    assert!(
        families[0]
            .iter()
            .any(|x| x.absolute() == second_child.e.absolute())
    );
}

#[test]
fn fresh_successors_only_include_non_stale_direct_children() {
    let mut lc = LifeCoreList::new();
    let root = lc.add("", text_body("root"), None).unwrap();
    let stale_child = lc.add("", text_body("stale-child"), Some(root.e)).unwrap();
    let fresh_grandchild = lc
        .add("", text_body("fresh-grandchild"), Some(stale_child.e))
        .unwrap();
    let fresh_child = lc.add("", text_body("fresh-child"), Some(root.e)).unwrap();

    let successors = lc.fresh_successors(root.e.absolute());
    assert_eq!(successors.len(), 1);
    assert_eq!(successors[0].absolute(), fresh_child.e.absolute());
    assert!(
        !successors
            .iter()
            .any(|x| x.absolute() == fresh_grandchild.e.absolute())
    );
}

#[test]
fn relative_degree_zero_for_nodes_with_same_parent() {
    let mut lc = LifeCoreList::new();
    let root = lc.add("", text_body("root"), None).unwrap();
    let left = lc.add("", text_body("left"), Some(root.e)).unwrap();
    let right = lc.add("", text_body("right"), Some(root.e)).unwrap();
    let grandchild = lc.add("", text_body("grandchild"), Some(left.e)).unwrap();

    assert_eq!(
        lc.relative_degree(left.e.absolute(), right.e.absolute()),
        Some(0)
    );
    assert!(lc.are_siblings(left.e.absolute(), right.e.absolute()));
    assert_eq!(
        lc.relative_degree(grandchild.e.absolute(), right.e.absolute()),
        Some(1)
    );
}

// TODO revise
#[cfg(never)]
#[test]
fn node_type_resolution_depends_on_context() {
    let mut lc = LifeCoreList::new();
    let old_type = lc
        .add(
            NodeType::name(),
            type_decl(
                "Thing",
                &[(
                    "old_field",
                    (PayloadType::Text, NodeTypeFieldStatus::Mandatory),
                )],
                &[],
                &[],
                &[],
            )
            .into(),
            None,
        )
        .unwrap();
    let types_before_override = lc
        .types(old_type.e.inclusive_left_items())
        .collect::<Vec<_>>();
    let _thing = lc
        .add(
            "Thing",
            NodeBodyExt::from(&[("old_field", PayloadExt::Text("old".to_string()))]),
            None,
        )
        .unwrap();
    let new_type = lc
        .add(
            NodeType::name(),
            type_decl(
                "Thing",
                &[(
                    "new_field",
                    (PayloadType::Number, NodeTypeFieldStatus::Mandatory),
                )],
                &[],
                &[],
                &[],
            )
            .into(),
            Some(old_type.e),
        )
        .unwrap();

    assert_eq!(types_before_override.len(), 1);
    assert_eq!(
        types_before_override[0].1.1.field[0].0.as_str(),
        "old_field"
    );
    assert!(lc.basic_validation().is_ok());
    assert_eq!(new_type.ty_name.as_str(), NodeType::name());
}

#[test]
fn context_intersection_uses_the_tighter_left_bound() {
    let left = EventContext {
        end: 8,
        thing: Some(5),
    };
    let right = EventContext {
        end: 7,
        thing: Some(5),
    };

    let intersection = left.intersect(&right);

    assert_eq!(intersection.end, 7);
    assert!(intersection.validate());
}

#[test]
fn context_intersection_treats_an_unbounded_end_as_infinity() {
    let bounded = EventContext {
        end: 4,
        thing: None,
    };

    let intersection = EventContext::universe().intersect(&bounded);

    assert_eq!(intersection, bounded);
}

#[test]
fn basic_validation_reports_an_out_of_range_parent_without_panicking() {
    let mut lc = LifeCoreList::new();
    lc.add("", text_body("node"), None).unwrap();
    let mut serialized = serde_json::to_value(lc).unwrap();
    serialized["event"][0]["Node"]["parent"] = serde_json::json!(99);
    let corrupted: LifeCoreList = serde_json::from_value(serialized).unwrap();

    let validation = std::panic::catch_unwind(|| corrupted.basic_validation());

    assert!(
        validation.is_ok(),
        "basic_validation must diagnose an out-of-range stored reference instead of indexing it"
    );
    assert!(validation.unwrap().is_err());
}

// TODO revise
#[cfg(never)]
#[test]
#[should_panic]
fn last_on_empty_lifecore_panics() {
    generic_last_on_empty_lifecore(LifeCoreList::new());
}

#[test]
#[should_panic]
fn get_with_unpointed_context_panics() {
    let lc = LifeCoreList::new();
    generic_get_with_unpointed_context_panics(&lc);
}

#[test]
#[should_panic]
fn window_with_unpointed_context_panics() {
    let lc = LifeCoreList::new();
    generic_window_with_unpointed_context_panics(&lc);
}

#[test]
fn node_on_edge_context() {
    let mut lc = LifeCoreList::new();
    let n0 = lc.add("", text_body("yellow"), None).unwrap();
    let n1 = lc.add("", text_body("orange"), None).unwrap();
    let edge = lc.connect(n0.e, "edge", n1.e).unwrap();

    generic_node_on_edge_context(&lc, edge.e);
}

// TODO revise
#[cfg(never)]
#[test]
#[should_panic]
fn edge_on_node_context_panics() {
    let mut lc = LifeCoreList::new();
    let node = lc.add("", text_body("yellow"), None).unwrap();

    generic_edge_on_node_context(&lc, node.e);
}

#[test]
#[should_panic]
fn is_stale_with_out_of_range_context_panics() {
    let mut lc = LifeCoreList::new();
    let _node = lc.add("", text_body("yellow"), None).unwrap();

    generic_is_stale_with_out_of_range_context_panics(&lc);
}
