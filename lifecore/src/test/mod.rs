//! Generic tests that check for panic scenarios are not easy to organize.
//! As a crutch, we will be abusing `std::panic::catch_unwind` to assert for
//! panic behavior This is quite bad, and needs to get refactored to more
//! reliable `#[should_panic]` tests

macro_rules! assert_panic {
    ($stmt:stmt$(,)?) => {
        ::std::panic::catch_unwind(|| -> () { $stmt })
            .expect_err("assert_panic! argument did not panic")
    };
}

use crate::{
    BasicValidationFailure,
    BasicValidationFailureScenario::{self, *},
    EventContext, LifeCore, LifeCoreError, LifeCoreList, NodeBodyExt, NodeType, NodeTypeClass,
    NodeTypeFieldStatus, PayloadExt, PayloadType, TypedBody, analysis,
};

fn simple_body(value: &str) -> NodeBodyExt {
    NodeBodyExt::from(&[("Hello", PayloadExt::Text(value.to_string()))])
}

fn generic_is_stale_with_out_of_range_context_panics<LC: LifeCore>(lc: &LC) {
    let _ = lc.is_stale(EventContext::universe().look_raw(usize::MAX / 2));
}

#[test]
fn new_is_valid() {
    let lc = LifeCoreList::default();
    assert!(lc.basic_validation().is_ok());
}

#[test]
fn basic_01() {
    let mut lc = LifeCoreList::default();
    let n0 = lc.add("", simple_body("yellow"), None).unwrap();
    let n1 = lc.add("", simple_body("orange"), None).unwrap();
    let _e0 = lc.connect(n0.e, "skrunkle", n1.e).unwrap();
    assert!(lc.basic_validation().is_ok());
}

#[test]
fn basic_02() {}

#[test]
fn no_dupe_node() {
    let mut lc = LifeCoreList::default();
    let _n0 = lc.add("", simple_body("yellow"), None).unwrap();
    let error = lc.add("", simple_body("yellow"), None).unwrap_err();
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
    let mut lc = LifeCoreList::default();
    let n0 = lc.add("", simple_body("yellow"), None).unwrap();
    let n1 = lc.add("", simple_body("orange"), None).unwrap();
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
    let mut lc = LifeCoreList::default();
    let n0 = lc.add("", simple_body("yellow"), None).unwrap();
    let _n1 = lc.add("", simple_body("yellow"), Some(n0.e)).unwrap();
    assert!(lc.basic_validation().is_ok());
}

#[test]
fn stale_rule_01() {
    let mut lc = LifeCoreList::default();
    let parent = lc.add("", simple_body("yellow"), None).unwrap();
    let child = lc.add("", simple_body("yellow"), Some(parent.e)).unwrap();
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
    let mut lc = LifeCoreList::default();
    let parent = lc.add("", simple_body("yellow"), None).unwrap();
    let child = lc.add("", simple_body("yellow"), Some(parent.e)).unwrap();
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
    basic01(LifeCoreList::default());
}

#[test]
fn families_include_root_and_children_after_overrides() {
    let mut lc = LifeCoreList::default();
    let root = lc.add("", simple_body("root"), None).unwrap();
    let child = lc.add("", simple_body("child"), Some(root.e)).unwrap();
    let second_child = lc
        .add("", simple_body("second-child"), Some(root.e))
        .unwrap();

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
fn relative_degree_zero_for_nodes_with_same_parent() {
    let mut lc = LifeCoreList::default();
    let root = lc.add("", simple_body("root"), None).unwrap();
    let left = lc.add("", simple_body("left"), Some(root.e)).unwrap();
    let right = lc.add("", simple_body("right"), Some(root.e)).unwrap();
    let grandchild = lc.add("", simple_body("grandchild"), Some(left.e)).unwrap();

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
    let mut lc = LifeCoreList::default();
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
    let mut lc = LifeCoreList::default();
    lc.add("", simple_body("node"), None).unwrap();
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

#[test]
#[should_panic]
fn get_with_unpointed_context_panics() {
    let lc = LifeCoreList::default();
    generic_get_with_unpointed_context_panics(&lc);
}

#[test]
#[should_panic]
fn window_with_unpointed_context_panics() {
    let lc = LifeCoreList::default();
    generic_window_with_unpointed_context_panics(&lc);
}

#[test]
fn node_on_edge_context() {
    let mut lc = LifeCoreList::default();
    let n0 = lc.add("", simple_body("yellow"), None).unwrap();
    let n1 = lc.add("", simple_body("orange"), None).unwrap();
    let edge = lc.connect(n0.e, "edge", n1.e).unwrap();

    generic_node_on_edge_context(&lc, edge.e);
}

#[test]
#[should_panic]
fn is_stale_with_out_of_range_context_panics() {
    let mut lc = LifeCoreList::default();
    let _node = lc.add("", simple_body("yellow"), None).unwrap();

    generic_is_stale_with_out_of_range_context_panics(&lc);
}

mod extra;
mod generic;
