//! Tests that are generic against LifeCore

use super::*;
use itertools::{self, Itertools};

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
            assert!(!lc.is_stale(parent.e).unwrap());
            assert!(lc.is_stale(parent.e.absolute()).unwrap());
            assert!(!lc.is_stale(first_child.e).unwrap());
            assert!(!lc.is_stale(second_child.e).unwrap());
            assert_eq!(
                lc.is_stale(first_child.e.absolute()).unwrap(),
                $current_children_are_stale
            );
            assert_eq!(
                lc.is_stale(second_child.e.absolute()).unwrap(),
                $current_children_are_stale
            );
            assert!(
                !lc.is_stale(initial_end_marker.look_at(first_child.e))
                    .unwrap()
            );
            assert!(
                !lc.is_stale(initial_end_marker.look_at(second_child.e))
                    .unwrap()
            );
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

pub(in crate::test) fn fresh_successors<LC: LifeCore>(mut lc: LC) {
    assert!(lc.last().is_none());
    let parent = lc.add("", simple_body("parent"), None).unwrap();
    itertools::assert_equal(
        lc.fresh_successors(parent.e)
            .into_iter()
            .map(|x| x.absolute()),
        [parent.e.absolute()],
    );
    let first_child = lc
        .add("", simple_body("first_child"), Some(parent.e))
        .unwrap();
    let second_child = lc
        .add("", simple_body("second_child"), Some(parent.e))
        .unwrap();
    itertools::assert_equal(
        lc.fresh_successors(lc.last().unwrap().look_at(parent.e))
            .into_iter()
            .map(|x| x.absolute())
            .sorted(),
        [first_child.e.absolute(), second_child.e.absolute()]
            .into_iter()
            .sorted(),
    );
}
