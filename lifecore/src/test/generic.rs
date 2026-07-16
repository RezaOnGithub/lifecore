//! Tests that are generic against LifeCore

use super::*;

pub(in crate::test) fn basic01<LC: LifeCore>(mut lc: LC) {
    assert!(lc.last().is_none());
    let parent = lc.add("", simple_body("parent"), None).unwrap();
    let first_child = lc
        .add("", simple_body("first_child"), Some(parent.e))
        .unwrap();
    let second_child = lc
        .add("", simple_body("second_child"), Some(parent.e))
        .unwrap();
    let initial_end_marker = lc.last().unwrap();
    macro_rules! old_state_validation {
        ($current_children_are_stale:expr) => {
            assert!(!lc.is_stale(parent.e));
            assert!(lc.is_stale(parent.e.absolute()));
            assert!(!lc.is_stale(first_child.e));
            assert!(!lc.is_stale(second_child.e));
            assert_eq!(
                lc.is_stale(first_child.e.absolute()),
                $current_children_are_stale
            );
            assert_eq!(
                lc.is_stale(second_child.e.absolute()),
                $current_children_are_stale
            );
            assert!(!lc.is_stale(initial_end_marker.look_at(first_child.e)));
            assert!(!lc.is_stale(initial_end_marker.look_at(second_child.e)));
        };
    }

    old_state_validation!(false);

    let child_of_first_child = lc
        .add("", simple_body("child_of_first_child"), Some(first_child.e))
        .unwrap();
    let _ = lc.connect(second_child.e, "Uncle_To", child_of_first_child.e);
    let _ = lc.add(
        "",
        simple_body("child_of_second_child"),
        Some(second_child.e),
    );

    // Nothing should change with the code using old Event Contexts;
    old_state_validation!(true);
}
