use itertools::Itertools;

use crate::{EdgeExt, EventContext, LifeCore, LifeCoreList, NodeBodyExt, NodeExt, PayloadExt};
use super::*;

#[test]
fn new_is_valid() {
    let lc = LifeCoreList::new();
    assert!(lc.basic_validation().is_success());
}

#[test]
fn basic_01() {
    let mut lc = LifeCoreList::new();
    let n0 = lc
        .add(
            "",
            NodeBodyExt::from(&[("date", PayloadExt::Text("yellow".to_string()))]),
            None,
        )
        .unwrap();
    let n1 = lc
        .add(
            "",
            NodeBodyExt::from(&[("date", PayloadExt::Text("orange".to_string()))]),
            None,
        )
        .unwrap();
    let _e0 = lc.connect(n0.e, "skrunkle", n1.e).unwrap();
    assert!(lc.basic_validation().is_success());
}



#[test]
fn basic_02() {}

#[test]
#[should_panic]
fn no_dupe_node() {
    let mut lc = LifeCoreList::new();
    let _n0 = lc
        .add(
            "",
            NodeBodyExt::from(&[("date", PayloadExt::Text("yellow".to_string()))]),
            None,
        )
        .unwrap();
    let _n1 = lc
        .add(
            "",
            NodeBodyExt::from(&[("date", PayloadExt::Text("yellow".to_string()))]),
            None,
        )
        .unwrap();
}

#[test]
#[should_panic]
fn no_dupe_edge() {
    let mut lc = LifeCoreList::new();
    let n0 = lc
        .add(
            "",
            NodeBodyExt::from(&[("date", PayloadExt::Text("yellow".to_string()))]),
            None,
        )
        .unwrap();
    let n1 = lc
        .add(
            "",
            NodeBodyExt::from(&[("date", PayloadExt::Text("orange".to_string()))]),
            None,
        )
        .unwrap();
    let e0 = lc.connect(n0.e, "skrunkle", n1.e).unwrap();
    let _e1 = lc.connect(n0.e, "skrunkle", n1.e).unwrap();
}

#[test]
fn dupe_parent_and_child() {
    let mut lc = LifeCoreList::new();
    let n0 = lc
        .add(
            "",
            NodeBodyExt::from(&[("date", PayloadExt::Text("yellow".to_string()))]),
            None,
        )
        .unwrap();
    let n1 = lc
        .add(
            "",
            NodeBodyExt::from(&[("date", PayloadExt::Text("yellow".to_string()))]),
            Some(n0.e),
        )
        .unwrap();
    assert!(lc.basic_validation().is_success());
}

#[test]
#[should_panic]
fn stale_rule_01() {
    let mut lc = LifeCoreList::new();
    let parent = lc
        .add(
            "",
            NodeBodyExt::from(&[("date", PayloadExt::Text("yellow".to_string()))]),
            None,
        )
        .unwrap();
    let child = lc
        .add(
            "",
            NodeBodyExt::from(&[("date", PayloadExt::Text("yellow".to_string()))]),
            Some(parent.e),
        )
        .unwrap();
    let _e = lc.connect(parent.e, "", child.e).unwrap();
}

#[test]
#[should_panic]
fn stale_rule_02() {
    let mut lc = LifeCoreList::new();
    let parent = lc
        .add(
            "",
            NodeBodyExt::from(&[("date", PayloadExt::Text("yellow".to_string()))]),
            None,
        )
        .unwrap();
    let child = lc
        .add(
            "",
            NodeBodyExt::from(&[("date", PayloadExt::Text("yellow".to_string()))]),
            Some(parent.e),
        )
        .unwrap();
    let _e = lc.connect(child.e, "", parent.e).unwrap();
}
