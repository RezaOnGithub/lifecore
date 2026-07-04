use crate::{
    analyzer::typecheck::{NodeType, describe_self},
    *,
};

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
            to_map(&[("date", PayloadExt::Text("yellow".to_string()))]),
            None,
        )
        .unwrap();
    let n1 = lc
        .add(
            "",
            to_map(&[("date", PayloadExt::Text("orange".to_string()))]),
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
            to_map(&[("date", PayloadExt::Text("yellow".to_string()))]),
            None,
        )
        .unwrap();
    let _n1 = lc
        .add(
            "",
            to_map(&[("date", PayloadExt::Text("yellow".to_string()))]),
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
            to_map(&[("date", PayloadExt::Text("yellow".to_string()))]),
            None,
        )
        .unwrap();
    let n1 = lc
        .add(
            "",
            to_map(&[("date", PayloadExt::Text("orange".to_string()))]),
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
            to_map(&[("date", PayloadExt::Text("yellow".to_string()))]),
            None,
        )
        .unwrap();
    let n1 = lc
        .add(
            "",
            to_map(&[("date", PayloadExt::Text("yellow".to_string()))]),
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
            to_map(&[("date", PayloadExt::Text("yellow".to_string()))]),
            None,
        )
        .unwrap();
    let child = lc
        .add(
            "",
            to_map(&[("date", PayloadExt::Text("yellow".to_string()))]),
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
            to_map(&[("date", PayloadExt::Text("yellow".to_string()))]),
            None,
        )
        .unwrap();
    let child = lc
        .add(
            "",
            to_map(&[("date", PayloadExt::Text("yellow".to_string()))]),
            Some(parent),
        )
        .unwrap();
    let _e = lc.connect(child, "", parent).unwrap();
}

#[test]
fn node_deserialization01() {
    let nt_describe_json = serde_json::to_string(&describe_self()).unwrap();
    eprintln!("nt_describe_json\t{}", nt_describe_json.as_str());

    let nt_describe_hashmap = serde_json::from_str::<NodeBody>(nt_describe_json.as_str()).unwrap();
    eprintln!("nt_describe_hashmap\t{:#?}", &nt_describe_hashmap);

    let _ = NodeType::try_from(&nt_describe_hashmap).unwrap();
}

#[test]
#[should_panic]
fn no_bootstrap_typecheck() {
    let mut lc = LifeCore::new();
    let _ = lc
        .add(
            "",
            to_map(&[("date", PayloadExt::Text("yellow".to_string()))]),
            None,
        )
        .unwrap();
    
}
