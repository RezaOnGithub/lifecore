#![cfg(test)]

use lifecore::{LifeCore, PayloadExt, NodeBody};

mod typecheck;

#[test]
fn new_is_valid() {
    let lc = LifeCore::new();
    assert!(lc.basic_validation().is_ok());
}

#[test]
fn basic_01() {
    let mut lc = LifeCore::new();
    let n0 = lc
        .add(
            "",
            NodeBody::from(&[("date", PayloadExt::Text("yellow".to_string()))]),
            None,
        )
        .unwrap();
    let n1 = lc
        .add(
            "",
            NodeBody::from(&[("date", PayloadExt::Text("orange".to_string()))]),
            None,
        )
        .unwrap();
    let _e0 = lc.connect(n0, "skrunkle", n1).unwrap();
    assert!(lc.basic_validation().is_ok());
}

#[test]
fn basic_02() {}

#[test]
#[should_panic]
fn no_dupe_node() {
    let mut lc = LifeCore::new();
    let _n0 = lc
        .add(
            "",
            NodeBody::from(&[("date", PayloadExt::Text("yellow".to_string()))]),
            None,
        )
        .unwrap();
    let _n1 = lc
        .add(
            "",
            NodeBody::from(&[("date", PayloadExt::Text("yellow".to_string()))]),
            None,
        )
        .unwrap();
}

#[test]
#[should_panic]
fn no_dupe_edge() {
    let mut lc = LifeCore::new();
    let n0 = lc
        .add(
            "",
            NodeBody::from(&[("date", PayloadExt::Text("yellow".to_string()))]),
            None,
        )
        .unwrap();
    let n1 = lc
        .add(
            "",
            NodeBody::from(&[("date", PayloadExt::Text("orange".to_string()))]),
            None,
        )
        .unwrap();
    let e0 = lc.connect(n0, "skrunkle", n1).unwrap();
    let _e1 = lc.connect(n0, "skrunkle", n1).unwrap();
}

#[test]
fn dupe_parent_and_child() {
    let mut lc = LifeCore::new();
    let n0 = lc
        .add(
            "",
            NodeBody::from(&[("date", PayloadExt::Text("yellow".to_string()))]),
            None,
        )
        .unwrap();
    let n1 = lc
        .add(
            "",
            NodeBody::from(&[("date", PayloadExt::Text("yellow".to_string()))]),
            Some(n0),
        )
        .unwrap();
    assert!(lc.basic_validation().is_ok());
}

#[test]
#[should_panic]
fn stale_rule_01() {
    let mut lc = LifeCore::new();
    let parent = lc
        .add(
            "",
            NodeBody::from(&[("date", PayloadExt::Text("yellow".to_string()))]),
            None,
        )
        .unwrap();
    let child = lc
        .add(
            "",
            NodeBody::from(&[("date", PayloadExt::Text("yellow".to_string()))]),
            Some(parent),
        )
        .unwrap();
    let _e = lc.connect(parent, "", child).unwrap();
}

#[test]
#[should_panic]
fn stale_rule_02() {
    let mut lc = LifeCore::new();
    let parent = lc
        .add(
            "",
            NodeBody::from(&[("date", PayloadExt::Text("yellow".to_string()))]),
            None,
        )
        .unwrap();
    let child = lc
        .add(
            "",
            NodeBody::from(&[("date", PayloadExt::Text("yellow".to_string()))]),
            Some(parent),
        )
        .unwrap();
    let _e = lc.connect(child, "", parent).unwrap();
}
